//! `SourceHub`: one controller owns the session at a time.
//!
//! The engine polls a single source. Since v2.6.0 two workers can produce
//! events — the HID worker (DualSense, Switch Pro) and the NS2 Pro BLE
//! worker — so the hub arbitrates between them:
//!
//! * The first source to emit `Connected` becomes the owner. While an owner
//!   exists, every event from the other source is dropped, its
//!   `Connected` / `Disconnected` included.
//! * The owner's `Disconnected` ends the ownership. If the other source is
//!   connected but ignored (it sent `Connected` while the owner held the
//!   session, and no `Disconnected` since), it is promoted at once: the hub
//!   emits a synthetic `Connected(kind)` with the kind it announced, right
//!   after the old owner's `Disconnected`, and passes its later events
//!   through. Buttons it already held are not replayed (the workers send
//!   diffs only), so the engine sees them released until their next change
//!   — the safe direction: never a phantom press.
//! * A `Disconnected` from a non-owner, or with no owner, is dropped: there
//!   is no session to end (the HID worker reports one after every attempt,
//!   even one that never connected). An ignored source that disconnects is
//!   simply no longer waiting. This holds for the single-source test fake
//!   too: engine tests must send `Connected` before a `Disconnected` they
//!   want the engine to see.
//! * With no owner, other events pass through (the engine's fake source in
//!   tests sends buttons without `Connected`).
//! * Commands (manual disconnect, rumble) reach only the owner's worker;
//!   with no owner they are dropped. They are routed by `service`, which
//!   the engine calls every tick even while paused (when it does not
//!   `poll`).
//! * While a source owns the session, a source that can be suspended (the
//!   BLE worker) is told to stop scanning: a continuous BLE scan shares the
//!   radio with the DualSense's Bluetooth link, and its finds would be
//!   ignored anyway.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};

use super::controller::ControllerKind;
use super::cursor_params::CursorParams;
use super::events::GamepadEvent;
use super::hid_source::HidSource;
use super::SourceCmd;
use super::ns2_ble::Ns2BleSource;

/// A worker the hub drains.
enum Src {
    Hid(HidSource),
    Ble(Ns2BleSource),
}

impl Src {
    fn poll(&mut self, out: &mut Vec<GamepadEvent>) {
        match self {
            Src::Hid(s) => s.poll(out),
            Src::Ble(s) => s.poll(out),
        }
    }
}

struct Slot {
    src: Src,
    /// Commands (manual disconnect, rumble) for this worker (`None`: the
    /// source takes none).
    cmd_tx: Option<Sender<SourceCmd>>,
    /// Set while another source owns the session (`None`: not suspendable).
    suspend: Option<Arc<AtomicBool>>,
    /// The kind the source announced while its session is open (sent
    /// `Connected`, no `Disconnected` yet), whether or not it owns. A
    /// non-owner with `Some` is connected but ignored: promoted when the
    /// owner leaves.
    session: Option<ControllerKind>,
}

impl Slot {
    fn new(src: Src, cmd_tx: Option<Sender<SourceCmd>>, suspend: Option<Arc<AtomicBool>>) -> Self {
        Slot { src, cmd_tx, suspend, session: None }
    }
}

pub struct SourceHub {
    /// Polled in order each tick; `[0]` is the HID worker.
    slots: Vec<Slot>,
    owner: Option<usize>,
    /// Commands from the engine handle (manual disconnect, rumble).
    cmd_rx: Option<Receiver<SourceCmd>>,
    scratch: Vec<GamepadEvent>,
}

impl SourceHub {
    /// Spawn the HID worker and the NS2 Pro BLE worker. `cmd_rx`
    /// carries the engine's commands (routed to the owner);
    /// `warn_tx` receives status warnings from the BLE worker.
    pub fn new(
        params: CursorParams,
        cmd_rx: Receiver<SourceCmd>,
        warn_tx: Sender<String>,
    ) -> Result<Self> {
        let (hid_tx, hid_rx) = crossbeam_channel::bounded::<SourceCmd>(4);
        let hid = HidSource::new(params, hid_rx)?;
        let (ble_tx, ble_rx) = crossbeam_channel::bounded::<SourceCmd>(4);
        let ble = Ns2BleSource::new(ble_rx, warn_tx)?;
        let suspend = ble.suspend_flag();
        Ok(Self::from_slots(
            vec![
                Slot::new(Src::Hid(hid), Some(hid_tx), None),
                Slot::new(Src::Ble(ble), Some(ble_tx), Some(suspend)),
            ],
            Some(cmd_rx),
        ))
    }

    /// Test-only: a hub over one fake source that drains `GamepadEvent`s
    /// from `rx`. No command routing. The ownership rules still
    /// apply: a `Disconnected` with no prior `Connected` is dropped, so a
    /// test that wants the engine's disconnect handling must send
    /// `Connected` first.
    #[doc(hidden)]
    pub fn fake(rx: Receiver<GamepadEvent>) -> Self {
        Self::from_slots(vec![Slot::new(Src::Hid(HidSource::fake(rx)), None, None)], None)
    }

    /// Test-only: `fake` plus command routing — commands on `cmd_rx`
    /// are forwarded to `source_cmd_tx` while the fake source owns the
    /// session.
    #[doc(hidden)]
    pub fn fake_with_disconnect(
        rx: Receiver<GamepadEvent>,
        cmd_rx: Receiver<SourceCmd>,
        source_cmd_tx: Sender<SourceCmd>,
    ) -> Self {
        Self::from_slots(
            vec![Slot::new(Src::Hid(HidSource::fake(rx)), Some(source_cmd_tx), None)],
            Some(cmd_rx),
        )
    }

    /// Test-only: a hub over two fake sources (`a` polled first, `b`
    /// suspendable like the BLE worker).
    #[doc(hidden)]
    pub fn from_fakes(a: Receiver<GamepadEvent>, b: Receiver<GamepadEvent>) -> Self {
        Self::from_slots(
            vec![
                Slot::new(Src::Hid(HidSource::fake(a)), None, None),
                Slot::new(Src::Hid(HidSource::fake(b)), None, Some(Arc::new(AtomicBool::new(false)))),
            ],
            None,
        )
    }

    fn from_slots(slots: Vec<Slot>, cmd_rx: Option<Receiver<SourceCmd>>) -> Self {
        SourceHub { slots, owner: None, cmd_rx, scratch: Vec::with_capacity(32) }
    }

    /// Control work that must not wait for `poll`: forward pending commands
    /// (manual disconnect, rumble) to the owner and re-sync the suspend flags. The engine
    /// calls it every tick, paused or not (it skips `poll` while paused,
    /// and Turn Off stays clickable then). Idempotent; non-blocking.
    pub fn service(&mut self) {
        self.route_commands();
        self.sync_suspend();
    }

    /// Drain pending events of the owning source (or of any source while
    /// none owns) into `out`. Non-blocking. Also does `service`'s work.
    pub fn poll(&mut self, out: &mut Vec<GamepadEvent>) {
        self.route_commands();
        let mut scratch = std::mem::take(&mut self.scratch);
        for i in 0..self.slots.len() {
            scratch.clear();
            self.slots[i].src.poll(&mut scratch);
            for ev in scratch.drain(..) {
                self.admit(i, ev, out);
            }
        }
        self.scratch = scratch;
        self.sync_suspend();
    }

    /// Forward each pending command to the owner's worker only.
    fn route_commands(&mut self) {
        let Some(rx) = &self.cmd_rx else { return };
        while let Ok(cmd) = rx.try_recv() {
            let tx = self.owner.and_then(|o| self.slots[o].cmd_tx.as_ref());
            match tx {
                Some(tx) => {
                    let _ = tx.try_send(cmd);
                }
                None => tracing::debug!(?cmd, "command with no controller connected; ignored"),
            }
        }
    }

    /// Ownership bookkeeping for one event of source `i`; pushes to `out`
    /// what reaches the engine.
    fn admit(&mut self, i: usize, ev: GamepadEvent, out: &mut Vec<GamepadEvent>) {
        let pass = match ev {
            GamepadEvent::Connected(kind) => {
                self.slots[i].session = Some(kind);
                match self.owner {
                    None => {
                        self.owner = Some(i);
                        true
                    }
                    Some(o) => {
                        if o != i {
                            tracing::info!(?kind, "another controller owns the session; ignoring this one");
                        }
                        o == i
                    }
                }
            }
            GamepadEvent::Disconnected => {
                self.slots[i].session = None;
                if self.owner == Some(i) {
                    out.push(ev);
                    self.owner = None;
                    self.promote_waiting(out);
                }
                return;
            }
            _ => match self.owner {
                Some(o) => o == i,
                None => self.slots[i].session.is_none(),
            },
        };
        if pass {
            out.push(ev);
        }
    }

    /// The owner just left: hand the session to a connected-but-ignored
    /// source, announcing it with the kind it reported.
    fn promote_waiting(&mut self, out: &mut Vec<GamepadEvent>) {
        if let Some((j, kind)) =
            self.slots.iter().enumerate().find_map(|(j, s)| s.session.map(|k| (j, k)))
        {
            tracing::info!(?kind, "previous controller left; the waiting one takes over");
            self.owner = Some(j);
            out.push(GamepadEvent::Connected(kind));
        }
    }

    /// A suspendable source is suspended exactly while another one owns.
    fn sync_suspend(&self) {
        for (i, slot) in self.slots.iter().enumerate() {
            if let Some(flag) = &slot.suspend {
                let suspended = self.owner.is_some_and(|o| o != i);
                flag.store(suspended, Ordering::SeqCst);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gamepad::ControllerKind;
    use crossbeam_channel::{unbounded, Sender};

    const DS: GamepadEvent = GamepadEvent::Connected(ControllerKind::DualSense);
    const NS2: GamepadEvent = GamepadEvent::Connected(ControllerKind::Ns2Pro);
    const OFF: GamepadEvent = GamepadEvent::Disconnected;

    struct Rig {
        hub: SourceHub,
        a: Sender<GamepadEvent>,
        b: Sender<GamepadEvent>,
        /// Manual disconnect requests (the engine handle's side).
        disconnect: Sender<SourceCmd>,
        /// What each worker received.
        a_dc: Receiver<SourceCmd>,
        b_dc: Receiver<SourceCmd>,
        b_suspend: Arc<AtomicBool>,
    }

    /// Two fake sources with disconnect channels; `b` is suspendable.
    fn rig() -> Rig {
        let (a, a_rx) = unbounded();
        let (b, b_rx) = unbounded();
        let (disconnect, disconnect_rx) = unbounded();
        let (a_dc_tx, a_dc) = unbounded();
        let (b_dc_tx, b_dc) = unbounded();
        let b_suspend = Arc::new(AtomicBool::new(false));
        let hub = SourceHub::from_slots(
            vec![
                Slot::new(Src::Hid(HidSource::fake(a_rx)), Some(a_dc_tx), None),
                Slot::new(Src::Hid(HidSource::fake(b_rx)), Some(b_dc_tx), Some(b_suspend.clone())),
            ],
            Some(disconnect_rx),
        );
        Rig { hub, a, b, disconnect, a_dc, b_dc, b_suspend }
    }

    fn poll(hub: &mut SourceHub) -> Vec<GamepadEvent> {
        let mut out = Vec::new();
        hub.poll(&mut out);
        out
    }

    fn send(tx: &Sender<GamepadEvent>, evs: &[GamepadEvent]) {
        for e in evs {
            tx.send(*e).unwrap();
        }
    }

    #[test]
    fn first_connected_wins() {
        let (a, a_rx) = unbounded();
        let (b, b_rx) = unbounded();
        let mut hub = SourceHub::from_fakes(a_rx, b_rx);

        send(&b, &[NS2, GamepadEvent::ButtonDown(30)]);
        assert_eq!(poll(&mut hub), vec![NS2, GamepadEvent::ButtonDown(30)]);

        // `a` connects while `b` owns: nothing of `a` gets through.
        send(&a, &[DS, GamepadEvent::ButtonDown(0)]);
        send(&b, &[GamepadEvent::ButtonUp(30)]);
        assert_eq!(poll(&mut hub), vec![GamepadEvent::ButtonUp(30)]);

        // Both connect in the same tick: the first polled (`a`) wins.
        let (a, a_rx) = unbounded();
        let (b, b_rx) = unbounded();
        let mut hub = SourceHub::from_fakes(a_rx, b_rx);
        send(&b, &[NS2, GamepadEvent::ButtonDown(1)]);
        send(&a, &[DS, GamepadEvent::ButtonDown(2)]);
        assert_eq!(poll(&mut hub), vec![DS, GamepadEvent::ButtonDown(2)]);
    }

    #[test]
    fn non_owner_events_dropped() {
        let (a, a_rx) = unbounded();
        let (b, b_rx) = unbounded();
        let mut hub = SourceHub::from_fakes(a_rx, b_rx);
        send(&a, &[DS]);
        assert_eq!(poll(&mut hub), vec![DS]);

        send(
            &b,
            &[
                NS2,
                GamepadEvent::ButtonDown(3),
                GamepadEvent::Stick { axis: 0, value: 1.0 },
                GamepadEvent::Trigger { axis: 4, value: 1.0 },
                GamepadEvent::ButtonUp(3),
                OFF,
            ],
        );
        send(&a, &[GamepadEvent::ButtonDown(5), GamepadEvent::MouseDelta { dx: 1, dy: 2 }]);
        // The non-owner's Disconnected must not end the owner's session.
        assert_eq!(
            poll(&mut hub),
            vec![GamepadEvent::ButtonDown(5), GamepadEvent::MouseDelta { dx: 1, dy: 2 }]
        );
        send(&a, &[GamepadEvent::ButtonUp(5)]);
        assert_eq!(poll(&mut hub), vec![GamepadEvent::ButtonUp(5)]);
    }

    #[test]
    fn owner_disconnect_releases_ownership() {
        let (a, a_rx) = unbounded();
        let (b, b_rx) = unbounded();
        let mut hub = SourceHub::from_fakes(a_rx, b_rx);
        send(&a, &[DS, GamepadEvent::ButtonDown(0), OFF]);
        assert_eq!(poll(&mut hub), vec![DS, GamepadEvent::ButtonDown(0), OFF]);

        // Nobody owns now: `b` may.
        send(&b, &[NS2, GamepadEvent::ButtonDown(31)]);
        assert_eq!(poll(&mut hub), vec![NS2, GamepadEvent::ButtonDown(31)]);
    }

    #[test]
    fn second_source_can_own_after_first_disconnects() {
        let (a, a_rx) = unbounded();
        let (b, b_rx) = unbounded();
        let mut hub = SourceHub::from_fakes(a_rx, b_rx);
        send(&a, &[DS]);
        send(&b, &[NS2]);
        assert_eq!(poll(&mut hub), vec![DS]);

        // `b`'s connected-but-ignored session ends while `a` owns.
        send(&b, &[OFF]);
        assert_eq!(poll(&mut hub), vec![]);

        // `a` leaves with nobody waiting: no owner.
        send(&a, &[OFF]);
        assert_eq!(poll(&mut hub), vec![OFF]);

        // `b` connects again and owns.
        send(&b, &[NS2, GamepadEvent::ButtonDown(1)]);
        assert_eq!(poll(&mut hub), vec![NS2, GamepadEvent::ButtonDown(1)]);

        // And `a` is now the ignored one.
        send(&a, &[DS, GamepadEvent::ButtonDown(0)]);
        assert_eq!(poll(&mut hub), vec![]);
    }

    #[test]
    fn ignored_pad_promoted_when_owner_disconnects() {
        // BLE owner leaves → the ignored HID pad takes over.
        let (a, a_rx) = unbounded();
        let (b, b_rx) = unbounded();
        let mut hub = SourceHub::from_fakes(a_rx, b_rx);
        send(&b, &[NS2]);
        assert_eq!(poll(&mut hub), vec![NS2]);
        send(&a, &[DS, GamepadEvent::ButtonDown(0)]);
        assert_eq!(poll(&mut hub), vec![]);
        send(&b, &[GamepadEvent::ButtonDown(30), OFF]);
        // Owner's Disconnected first (engine releases), then the waiting
        // pad's Connected with the kind it announced. Its held button 0 is
        // not replayed.
        assert_eq!(poll(&mut hub), vec![GamepadEvent::ButtonDown(30), OFF, DS]);
        send(&a, &[GamepadEvent::ButtonUp(0), GamepadEvent::ButtonDown(2)]);
        assert_eq!(poll(&mut hub), vec![GamepadEvent::ButtonUp(0), GamepadEvent::ButtonDown(2)]);
        // `b` is now the non-owner.
        send(&b, &[NS2, GamepadEvent::ButtonDown(31)]);
        assert_eq!(poll(&mut hub), vec![]);

        // HID owner leaves → the ignored BLE pad takes over (same tick:
        // `a` is polled first, so its Disconnected is seen before `b`'s events).
        send(&a, &[OFF]);
        send(&b, &[GamepadEvent::ButtonUp(31)]);
        assert_eq!(poll(&mut hub), vec![OFF, NS2, GamepadEvent::ButtonUp(31)]);
        // Kind is the announced one (a Switch Pro on HID, say).
        let (a, a_rx) = unbounded();
        let (b, b_rx) = unbounded();
        let mut hub = SourceHub::from_fakes(a_rx, b_rx);
        send(&b, &[NS2]);
        poll(&mut hub);
        send(&a, &[GamepadEvent::Connected(ControllerKind::NsPro)]);
        poll(&mut hub);
        send(&b, &[OFF]);
        assert_eq!(poll(&mut hub), vec![OFF, GamepadEvent::Connected(ControllerKind::NsPro)]);
    }

    #[test]
    fn ignored_pad_that_disconnected_is_not_promoted() {
        let (a, a_rx) = unbounded();
        let (b, b_rx) = unbounded();
        let mut hub = SourceHub::from_fakes(a_rx, b_rx);
        send(&a, &[DS]);
        send(&b, &[NS2, GamepadEvent::ButtonDown(30), OFF]);
        assert_eq!(poll(&mut hub), vec![DS], "ignored pad's Disconnected emits nothing");
        send(&a, &[OFF]);
        assert_eq!(poll(&mut hub), vec![OFF], "nobody waiting: no synthetic Connected");
        // `b` sends nothing more until it reconnects; a fresh session owns.
        send(&b, &[NS2]);
        assert_eq!(poll(&mut hub), vec![NS2]);
    }

    #[test]
    fn promotion_updates_ble_suspend() {
        // HID promoted → BLE suspended.
        let mut r = rig();
        send(&r.b, &[NS2]);
        poll(&mut r.hub);
        assert!(!r.b_suspend.load(Ordering::SeqCst), "BLE owns: not suspended");
        send(&r.a, &[DS]);
        poll(&mut r.hub);
        assert!(!r.b_suspend.load(Ordering::SeqCst), "BLE still owns");
        send(&r.b, &[OFF]);
        assert_eq!(poll(&mut r.hub), vec![OFF, DS]);
        assert!(r.b_suspend.load(Ordering::SeqCst), "HID promoted: BLE suspended");

        // BLE promoted → BLE not suspended.
        let mut r = rig();
        send(&r.a, &[DS]);
        poll(&mut r.hub);
        assert!(r.b_suspend.load(Ordering::SeqCst));
        // A BLE session already under way when HID took over.
        send(&r.b, &[NS2]);
        poll(&mut r.hub);
        send(&r.a, &[OFF]);
        assert_eq!(poll(&mut r.hub), vec![OFF, NS2]);
        assert!(!r.b_suspend.load(Ordering::SeqCst), "BLE promoted: not suspended");
        // Manual disconnect now goes to the promoted owner only.
        r.disconnect.send(SourceCmd::Disconnect).unwrap();
        poll(&mut r.hub);
        assert_eq!(r.b_dc.try_iter().count(), 1);
        assert_eq!(r.a_dc.try_iter().count(), 0);
    }

    #[test]
    fn manual_disconnect_routed_to_owner_only() {
        let mut r = rig();
        // No owner: the request is dropped.
        r.disconnect.send(SourceCmd::Disconnect).unwrap();
        assert_eq!(poll(&mut r.hub), vec![]);
        assert!(r.a_dc.try_recv().is_err() && r.b_dc.try_recv().is_err());

        // `b` owns (and `a` is connected but ignored): only `b` is told.
        send(&r.b, &[NS2]);
        assert_eq!(poll(&mut r.hub), vec![NS2]);
        send(&r.a, &[DS]);
        assert_eq!(poll(&mut r.hub), vec![]);
        r.disconnect.send(SourceCmd::Disconnect).unwrap();
        poll(&mut r.hub);
        assert_eq!(r.b_dc.try_iter().count(), 1);
        assert_eq!(r.a_dc.try_iter().count(), 0, "a connected-but-ignored pad stays on");

        // `b` goes; the waiting `a` is promoted and owns: only `a` is told.
        send(&r.b, &[OFF]);
        assert_eq!(poll(&mut r.hub), vec![OFF, DS]);
        r.disconnect.send(SourceCmd::Disconnect).unwrap();
        poll(&mut r.hub);
        assert_eq!(r.a_dc.try_iter().count(), 1);
        assert_eq!(r.b_dc.try_iter().count(), 0);
    }

    #[test]
    fn rumble_goes_to_owner_only() {
        let mut r = rig();
        send(&r.a, &[DS]);
        assert_eq!(poll(&mut r.hub), vec![DS]);
        send(&r.b, &[NS2]);
        assert_eq!(poll(&mut r.hub), vec![]);
        r.disconnect.send(SourceCmd::Rumble).unwrap();
        r.hub.service();
        assert_eq!(r.a_dc.try_iter().collect::<Vec<_>>(), vec![SourceCmd::Rumble]);
        assert!(r.b_dc.try_recv().is_err());
    }

    #[test]
    fn rumble_without_owner_is_dropped() {
        let mut r = rig();
        r.disconnect.send(SourceCmd::Rumble).unwrap();
        r.hub.service();
        assert!(r.a_dc.try_recv().is_err() && r.b_dc.try_recv().is_err());
    }

    #[test]
    fn promoted_pad_receives_rumble() {
        let mut r = rig();
        send(&r.a, &[DS]);
        poll(&mut r.hub);
        send(&r.b, &[NS2]);
        poll(&mut r.hub);
        send(&r.a, &[OFF]);
        assert_eq!(poll(&mut r.hub), vec![OFF, NS2]);
        r.disconnect.send(SourceCmd::Rumble).unwrap();
        r.hub.service();
        assert_eq!(r.b_dc.try_iter().collect::<Vec<_>>(), vec![SourceCmd::Rumble]);
        assert!(r.a_dc.try_recv().is_err());
    }

    #[test]
    fn disconnect_still_routes_to_owner() {
        let mut r = rig();
        send(&r.a, &[DS]);
        poll(&mut r.hub);
        r.disconnect.send(SourceCmd::Disconnect).unwrap();
        r.hub.service();
        assert_eq!(r.a_dc.try_iter().collect::<Vec<_>>(), vec![SourceCmd::Disconnect]);
        assert!(r.b_dc.try_recv().is_err());
    }

    #[test]
    fn disconnect_routed_without_poll() {
        // The engine skips `poll` while paused but still calls `service`.
        let mut r = rig();
        send(&r.a, &[DS]);
        assert_eq!(poll(&mut r.hub), vec![DS]);
        send(&r.a, &[GamepadEvent::ButtonDown(0)]);
        r.disconnect.send(SourceCmd::Disconnect).unwrap();
        r.hub.service();
        assert_eq!(r.a_dc.try_iter().count(), 1, "owner told without a poll");
        assert_eq!(r.b_dc.try_iter().count(), 0);
        // `service` routes control only; events wait for `poll`.
        assert_eq!(poll(&mut r.hub), vec![GamepadEvent::ButtonDown(0)]);
        // Idempotent: nothing pending → nothing forwarded.
        r.hub.service();
        poll(&mut r.hub);
        assert_eq!(r.a_dc.try_iter().count(), 0);
    }

    #[test]
    fn no_owner_disconnect_dropped() {
        let (a, a_rx) = unbounded();
        let (b, b_rx) = unbounded();
        let mut hub = SourceHub::from_fakes(a_rx, b_rx);
        // The HID worker reports Disconnected after a pad that never streamed.
        send(&a, &[OFF]);
        send(&b, &[OFF]);
        assert_eq!(poll(&mut hub), vec![]);
        // Nothing was disturbed: a later session is normal.
        send(&a, &[DS, OFF]);
        assert_eq!(poll(&mut hub), vec![DS, OFF]);
    }

    #[test]
    fn ble_suspended_while_hid_owns_and_resumed_after() {
        let mut r = rig();
        poll(&mut r.hub);
        assert!(!r.b_suspend.load(Ordering::SeqCst), "nobody owns: BLE scans");

        send(&r.a, &[DS]);
        poll(&mut r.hub);
        assert!(r.b_suspend.load(Ordering::SeqCst), "HID owns: BLE suspended");

        send(&r.a, &[GamepadEvent::ButtonDown(0)]);
        poll(&mut r.hub);
        assert!(r.b_suspend.load(Ordering::SeqCst), "still suspended while HID owns");

        send(&r.a, &[OFF]);
        poll(&mut r.hub);
        assert!(!r.b_suspend.load(Ordering::SeqCst), "HID gone: BLE resumes");

        // BLE owning never suspends BLE.
        send(&r.b, &[NS2]);
        poll(&mut r.hub);
        assert!(!r.b_suspend.load(Ordering::SeqCst));
    }

    /// One deliberate difference from the bare `HidSource`: a
    /// `Disconnected` with no prior `Connected` is dropped (see
    /// `no_owner_disconnect_dropped`), so tests of the engine's disconnect
    /// handling send `Connected` first.
    #[test]
    fn dualsense_only_unchanged() {
        let (tx, rx) = unbounded();
        let mut hub = SourceHub::fake(rx);
        let session = [
            DS,
            GamepadEvent::ButtonDown(0),
            GamepadEvent::Stick { axis: 0, value: 0.5 },
            GamepadEvent::Trigger { axis: 5, value: 1.0 },
            GamepadEvent::MouseDelta { dx: -3, dy: 4 },
            GamepadEvent::TouchpadClick { raw_x: 10, raw_y: 20, quadrant: 25 },
            GamepadEvent::TouchpadHover { raw_x: 10, raw_y: 20, quadrant: 25 },
            GamepadEvent::ButtonUp(0),
            OFF,
        ];
        // Every event of a session passes, in order, across reconnects.
        for _ in 0..2 {
            send(&tx, &session);
            assert_eq!(poll(&mut hub), session.to_vec());
        }
        // A DualSense that switches to a Switch Pro mid-run is the same source.
        send(&tx, &[GamepadEvent::Connected(ControllerKind::NsPro), OFF]);
        assert_eq!(
            poll(&mut hub),
            vec![GamepadEvent::Connected(ControllerKind::NsPro), OFF]
        );
        // Events without a Connected (engine tests' fake pad) still pass.
        send(&tx, &[GamepadEvent::ButtonDown(7), GamepadEvent::ButtonUp(7)]);
        assert_eq!(poll(&mut hub), vec![GamepadEvent::ButtonDown(7), GamepadEvent::ButtonUp(7)]);
    }
}
