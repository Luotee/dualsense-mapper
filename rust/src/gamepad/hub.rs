//! `SourceHub`: one controller owns the session at a time.
//!
//! The engine polls a single source. Since v2.6.0 two workers can produce
//! events — the HID worker (DualSense, Switch Pro) and the NS2 Pro BLE
//! worker — so the hub arbitrates between them:
//!
//! * The first source to emit `Connected` becomes the owner. While an owner
//!   exists, every event from the other source is dropped, its
//!   `Connected` / `Disconnected` included.
//! * The owner's `Disconnected` ends the ownership. A source whose
//!   `Connected` was dropped stays ignored until it disconnects and connects
//!   again (its pad state never reached the engine, so its later events
//!   would be diffs against nothing).
//! * A `Disconnected` from a non-owner, or with no owner, is dropped: there
//!   is no session to end (the HID worker reports one after every attempt,
//!   even one that never connected).
//! * With no owner, other events pass through (the engine's fake source in
//!   tests sends buttons without `Connected`).
//! * A manual disconnect reaches only the owner's worker; with no owner it
//!   is dropped.
//! * While a source owns the session, a source that can be suspended (the
//!   BLE worker) is told to stop scanning: a continuous BLE scan shares the
//!   radio with the DualSense's Bluetooth link, and its finds would be
//!   ignored anyway.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};

use super::cursor_params::CursorParams;
use super::events::GamepadEvent;
use super::hid_source::HidSource;
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
    /// Manual disconnect for this worker (`None`: the source has none).
    disconnect_tx: Option<Sender<()>>,
    /// Set while another source owns the session (`None`: not suspendable).
    suspend: Option<Arc<AtomicBool>>,
    /// The source has a session open (sent `Connected`, no `Disconnected`
    /// yet), whether or not it owns.
    live: bool,
}

impl Slot {
    fn new(src: Src, disconnect_tx: Option<Sender<()>>, suspend: Option<Arc<AtomicBool>>) -> Self {
        Slot { src, disconnect_tx, suspend, live: false }
    }
}

pub struct SourceHub {
    /// Polled in order each tick; `[0]` is the HID worker.
    slots: Vec<Slot>,
    owner: Option<usize>,
    /// Manual disconnect requests from the engine handle.
    disconnect_rx: Option<Receiver<()>>,
    scratch: Vec<GamepadEvent>,
}

impl SourceHub {
    /// Spawn the HID worker and the NS2 Pro BLE worker. `disconnect_rx`
    /// carries manual disconnect requests (routed to the owner);
    /// `warn_tx` receives status warnings from the BLE worker.
    pub fn new(
        params: CursorParams,
        disconnect_rx: Receiver<()>,
        warn_tx: Sender<String>,
    ) -> Result<Self> {
        let (hid_tx, hid_rx) = crossbeam_channel::bounded::<()>(4);
        let hid = HidSource::new(params, hid_rx)?;
        let (ble_tx, ble_rx) = crossbeam_channel::bounded::<()>(4);
        let ble = Ns2BleSource::new(ble_rx, warn_tx)?;
        let suspend = ble.suspend_flag();
        Ok(Self::from_slots(
            vec![
                Slot::new(Src::Hid(hid), Some(hid_tx), None),
                Slot::new(Src::Ble(ble), Some(ble_tx), Some(suspend)),
            ],
            Some(disconnect_rx),
        ))
    }

    /// Test-only: a hub over one fake source that drains `GamepadEvent`s
    /// from `rx`. No manual disconnect routing.
    #[doc(hidden)]
    pub fn fake(rx: Receiver<GamepadEvent>) -> Self {
        Self::from_slots(vec![Slot::new(Src::Hid(HidSource::fake(rx)), None, None)], None)
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

    fn from_slots(slots: Vec<Slot>, disconnect_rx: Option<Receiver<()>>) -> Self {
        SourceHub { slots, owner: None, disconnect_rx, scratch: Vec::with_capacity(32) }
    }

    /// Drain pending events of the owning source (or of any source while
    /// none owns) into `out`. Non-blocking.
    pub fn poll(&mut self, out: &mut Vec<GamepadEvent>) {
        self.route_disconnects();
        let mut scratch = std::mem::take(&mut self.scratch);
        for i in 0..self.slots.len() {
            scratch.clear();
            self.slots[i].src.poll(&mut scratch);
            for ev in scratch.drain(..) {
                if self.admit(i, &ev) {
                    out.push(ev);
                }
            }
        }
        self.scratch = scratch;
        self.sync_suspend();
    }

    /// Forward each pending manual disconnect to the owner's worker only.
    fn route_disconnects(&mut self) {
        let Some(rx) = &self.disconnect_rx else { return };
        while rx.try_recv().is_ok() {
            let tx = self.owner.and_then(|o| self.slots[o].disconnect_tx.as_ref());
            match tx {
                Some(tx) => {
                    let _ = tx.try_send(());
                }
                None => tracing::debug!("manual disconnect with no controller connected; ignored"),
            }
        }
    }

    /// Ownership bookkeeping for one event of source `i`; true if it
    /// reaches the engine.
    fn admit(&mut self, i: usize, ev: &GamepadEvent) -> bool {
        match ev {
            GamepadEvent::Connected(_) => {
                self.slots[i].live = true;
                match self.owner {
                    None => {
                        self.owner = Some(i);
                        true
                    }
                    Some(o) => {
                        if o != i {
                            tracing::info!(?ev, "another controller owns the session; ignoring this one");
                        }
                        o == i
                    }
                }
            }
            GamepadEvent::Disconnected => {
                self.slots[i].live = false;
                if self.owner == Some(i) {
                    self.owner = None;
                    true
                } else {
                    false
                }
            }
            _ => match self.owner {
                Some(o) => o == i,
                None => !self.slots[i].live,
            },
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
        disconnect: Sender<()>,
        /// What each worker received.
        a_dc: Receiver<()>,
        b_dc: Receiver<()>,
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

        // `a` leaves. `b`'s session began while `a` owned, so the engine
        // never saw its Connected: it stays ignored, even with no owner.
        send(&a, &[OFF]);
        assert_eq!(poll(&mut hub), vec![OFF]);
        send(&b, &[GamepadEvent::ButtonDown(1), GamepadEvent::Stick { axis: 1, value: -1.0 }]);
        assert_eq!(poll(&mut hub), vec![]);

        // Once `b` disconnects and connects again, it owns.
        send(&b, &[OFF]);
        assert_eq!(poll(&mut hub), vec![], "no-owner Disconnected is dropped");
        send(&b, &[NS2, GamepadEvent::ButtonDown(1)]);
        assert_eq!(poll(&mut hub), vec![NS2, GamepadEvent::ButtonDown(1)]);

        // And `a` is now the ignored one.
        send(&a, &[DS, GamepadEvent::ButtonDown(0)]);
        assert_eq!(poll(&mut hub), vec![]);
    }

    #[test]
    fn manual_disconnect_routed_to_owner_only() {
        let mut r = rig();
        // No owner: the request is dropped.
        r.disconnect.send(()).unwrap();
        assert_eq!(poll(&mut r.hub), vec![]);
        assert!(r.a_dc.try_recv().is_err() && r.b_dc.try_recv().is_err());

        // `b` owns (and `a` is connected but ignored): only `b` is told.
        send(&r.b, &[NS2]);
        assert_eq!(poll(&mut r.hub), vec![NS2]);
        send(&r.a, &[DS]);
        assert_eq!(poll(&mut r.hub), vec![]);
        r.disconnect.send(()).unwrap();
        poll(&mut r.hub);
        assert_eq!(r.b_dc.try_iter().count(), 1);
        assert_eq!(r.a_dc.try_iter().count(), 0, "a connected-but-ignored pad stays on");

        // `b` goes; `a` reconnects and owns: only `a` is told.
        send(&r.b, &[OFF]);
        assert_eq!(poll(&mut r.hub), vec![OFF]);
        send(&r.a, &[OFF, DS]);
        assert_eq!(poll(&mut r.hub), vec![DS]);
        r.disconnect.send(()).unwrap();
        poll(&mut r.hub);
        assert_eq!(r.a_dc.try_iter().count(), 1);
        assert_eq!(r.b_dc.try_iter().count(), 0);
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
