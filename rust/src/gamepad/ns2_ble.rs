//! Switch 2 Pro Controller (NS2 Pro) BLE transport.
//!
//! Windows: `Ns2BleSource` spawns one std thread ("ns2-ble") that runs a
//! current-thread tokio runtime driving btleplug through
//! Scanning → Connecting → Initializing → Streaming, and back to Scanning
//! on any failure, lost link or manual disconnect. The hardware-proven
//! connect sequence (spec §12) is: `connect()`, *immediately*
//! `request_connection_parameters(ThroughputOptimized)`, 500 ms pause,
//! `discover_services()`, response notify, optional `0x15` pairing,
//! stick calibration, player LED, input notify. OS SMP pairing is never
//! triggered.
//!
//! Other platforms: a stub with the same API that never connects.
//!
//! The decisions (advert acceptance, watchdog, rescan pacing, the
//! Connected/Disconnected contract of iron rule 11) are pure functions
//! below so they are unit-tested on every platform; the btleplug glue in
//! `imp` stays thin.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};

use super::controller::ControllerKind;
use super::events::GamepadEvent;
use super::ns2_protocol::{classify_advert, decode_05, parse_advert, AdvertMatch, Ns2StickCal, NS2_COMPANY_ID};
use super::pad::{emit_pad_diff, PadState, BUTTON_SLOTS};

/// No decoded report for this long → the link is considered lost.
pub(crate) const WATCHDOG_LIMIT: Duration = Duration::from_millis(1000);

/// Status warning for Windows builds older than 22000 (Windows 10), whose
/// BLE stack cannot reach the 15 ms connection interval.
pub const WARN_BLE_RATE_LIMITED_WIN10: &str = "ble_rate_limited_win10";

/// First Windows 11 build.
const FIRST_UNLIMITED_BUILD: u32 = 22000;

/// Pads worth connecting to: an NS2 Pro advert (company `0x0553`) that is
/// bonded to this adapter or in pairing mode. `local_msb_first` is the
/// adapter address as btleplug reports it.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn accept_advert(
    manufacturer_data: &HashMap<u16, Vec<u8>>,
    local_msb_first: [u8; 6],
) -> Option<AdvertMatch> {
    let ad = parse_advert(manufacturer_data.get(&NS2_COMPANY_ID)?)?;
    match classify_advert(&ad, local_msb_first) {
        AdvertMatch::Other => None,
        m => Some(m),
    }
}

/// Times the gap since the last decoded report.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct Watchdog {
    last: Instant,
    limit: Duration,
}

#[cfg_attr(not(windows), allow(dead_code))]
impl Watchdog {
    pub(crate) fn new(now: Instant, limit: Duration) -> Self {
        Watchdog { last: now, limit }
    }

    pub(crate) fn feed(&mut self, now: Instant) {
        self.last = now;
    }

    pub(crate) fn expired(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.last) >= self.limit
    }

    pub(crate) fn remaining(&self, now: Instant) -> Duration {
        self.limit.saturating_sub(now.saturating_duration_since(self.last))
    }
}

/// Why a connection attempt or session ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) enum SessionEnd {
    /// No adapter, scan error, or a connect / init step failed or timed out.
    Failed,
    /// Watchdog expired or btleplug reported `DeviceDisconnected`.
    LinkLost,
    /// Manual disconnect: the pad was told to turn off.
    TurnedOff,
    /// The source was dropped.
    Stopped,
}

/// Pause before scanning again. Never zero after a failure, so a pad that
/// keeps dropping (e.g. paired in Windows settings, where SMP kicks it off)
/// costs one attempt every 2 s instead of a busy loop.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn rescan_delay(end: SessionEnd) -> Duration {
    match end {
        SessionEnd::Failed | SessionEnd::LinkLost => Duration::from_secs(2),
        // Long enough that the pad we just turned off is asleep before we
        // look again (it may still advertise for a moment).
        SessionEnd::TurnedOff => Duration::from_secs(3),
        SessionEnd::Stopped => Duration::ZERO,
    }
}

/// True for Windows builds whose BLE stack is rate limited (Windows 10).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn ble_rate_limited(build: u32) -> bool {
    build < FIRST_UNLIMITED_BUILD
}

/// Per-connection input state: decodes 0x05 reports, emits the event
/// diff, and keeps iron rule 11 — `Connected` on the first decoded report,
/// `Disconnected` only if `Connected` was sent.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct InputStream {
    cal: Ns2StickCal,
    announced: bool,
    prev: Option<PadState>,
    prev_buttons: [bool; BUTTON_SLOTS],
    watchdog: Option<Watchdog>,
}

#[cfg_attr(not(windows), allow(dead_code))]
impl InputStream {
    pub(crate) fn new(cal: Ns2StickCal) -> Self {
        InputStream { cal, announced: false, prev: None, prev_buttons: [false; BUTTON_SLOTS], watchdog: None }
    }

    pub(crate) fn set_cal(&mut self, cal: Ns2StickCal) {
        self.cal = cal;
    }

    /// Start the watchdog (input notify just enabled).
    pub(crate) fn arm(&mut self, now: Instant) {
        self.watchdog = Some(Watchdog::new(now, WATCHDOG_LIMIT));
    }

    pub(crate) fn announced(&self) -> bool {
        self.announced
    }

    /// True once armed and no decoded report arrived for `WATCHDOG_LIMIT`.
    pub(crate) fn expired(&self, now: Instant) -> bool {
        self.watchdog.is_some_and(|w| w.expired(now))
    }

    /// Time until the watchdog fires (`None` while unarmed).
    pub(crate) fn remaining(&self, now: Instant) -> Option<Duration> {
        self.watchdog.map(|w| w.remaining(now))
    }

    /// Handle one input notification. Returns whether it decoded; only a
    /// decoded report feeds the watchdog.
    pub(crate) fn on_report(&mut self, buf: &[u8], now: Instant, tx: &Sender<GamepadEvent>) -> bool {
        let Some(s) = decode_05(buf, &self.cal) else {
            return false;
        };
        if !self.announced {
            let _ = tx.send(GamepadEvent::Connected(ControllerKind::Ns2Pro));
            self.announced = true;
        }
        emit_pad_diff(self.prev.as_ref(), &s, &mut self.prev_buttons, tx);
        self.prev = Some(s);
        if let Some(w) = self.watchdog.as_mut() {
            w.feed(now);
        }
        true
    }

    /// Session over: `Disconnected` iff `Connected` was sent.
    pub(crate) fn finish(&self, tx: &Sender<GamepadEvent>) {
        if self.announced {
            let _ = tx.send(GamepadEvent::Disconnected);
        }
    }
}

// ─── Windows: btleplug worker ──────────────────────────────────────────────

#[cfg(windows)]
pub struct Ns2BleSource {
    rx: Receiver<GamepadEvent>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(windows)]
impl Ns2BleSource {
    /// Spawn the BLE worker thread. It emits nothing until a pad's first
    /// decoded report. `disconnect_rx` carries manual disconnect requests
    /// (ignored while no pad is connected); `warn_tx` receives status
    /// warnings (`WARN_BLE_RATE_LIMITED_WIN10`, at most once).
    pub fn new(disconnect_rx: Receiver<()>, warn_tx: Sender<String>) -> anyhow::Result<Self> {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;
        let (tx, rx) = crossbeam_channel::unbounded();
        let stop = Arc::new(AtomicBool::new(false));
        let ctx = imp::Ctx { tx, stop: stop.clone(), disconnect_rx };
        std::thread::Builder::new()
            .name("ns2-ble".into())
            .spawn(move || imp::run(ctx, warn_tx))
            .map_err(|e| anyhow::anyhow!("spawning NS2 BLE worker thread: {e}"))?;
        Ok(Self { rx, stop })
    }

    /// Drain pending events into `out`. Non-blocking.
    pub fn poll(&mut self, out: &mut Vec<GamepadEvent>) {
        out.extend(self.rx.try_iter());
    }
}

#[cfg(windows)]
impl Drop for Ns2BleSource {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use crate::gamepad::ns2_protocol::{
        command, expected_b2, is_response_to, ltk, pairing_addresses, pairing_key_cmd,
        pairing_response_key, parse_stick_cal, spi_read_cmd, spi_read_payload, PairingKeys, CMD_LED,
        CMD_PAIR, CMD_POWER, SPI_CAL_LEN, SPI_LEFT_STICK_CAL, SPI_RIGHT_STICK_CAL, SUB_LED_SET,
        SUB_PAIR_A1, SUB_PAIR_A2, SUB_PAIR_ADDR, SUB_PAIR_COMMIT, SUB_POWER_OFF,
    };
    use anyhow::{anyhow, bail, Context, Result};
    use btleplug::api::{
        Central, CentralEvent, Characteristic, ConnectionParameterPreset, Manager as _,
        Peripheral as _, ScanFilter, ValueNotification, WriteType,
    };
    use btleplug::platform::{Adapter, Manager, Peripheral};
    use futures::{Stream, StreamExt};
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// Input report 0x05 notify.
    const UUID_INPUT: u128 = 0xab7de9be_89fe_49ad_828f_118f09df7fd2;
    /// Command write-without-response.
    const UUID_CMD: u128 = 0x649d4ac9_8eb7_4e6c_af44_1ea54fe5f005;
    /// Command response notify.
    const UUID_RESP: u128 = 0xc765a961_d9d8_4d36_a20a_5315b111836a;

    /// Upper bound on any wait before the stop flag is re-checked.
    const TICK: Duration = Duration::from_millis(100);
    const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
    const DISCOVER_TIMEOUT: Duration = Duration::from_secs(10);
    const GATT_OP_TIMEOUT: Duration = Duration::from_secs(5);
    const RESPONSE_TIMEOUT: Duration = Duration::from_millis(1500);
    /// After the connection-parameter request (hardware-proven value).
    const PARAMS_SETTLE: Duration = Duration::from_millis(500);
    /// Between the turn-off command and dropping the link, so the
    /// write-without-response leaves the radio first.
    const TURN_OFF_SETTLE: Duration = Duration::from_millis(500);
    const CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);
    const ADAPTER_RETRY: Duration = Duration::from_secs(2);
    const LED_PLAYER_1: [u8; 8] = [0x01, 0, 0, 0, 0, 0, 0, 0];

    type Events = Pin<Box<dyn Stream<Item = CentralEvent> + Send>>;
    type Notes = Pin<Box<dyn Stream<Item = ValueNotification> + Send>>;

    pub(super) struct Ctx {
        pub(super) tx: Sender<GamepadEvent>,
        pub(super) stop: Arc<AtomicBool>,
        pub(super) disconnect_rx: Receiver<()>,
    }

    impl Ctx {
        fn stopped(&self) -> bool {
            self.stop.load(Ordering::SeqCst)
        }

        /// A manual disconnect while no pad is connected is a no-op.
        fn drain_disconnects(&self) {
            while self.disconnect_rx.try_recv().is_ok() {}
        }
    }

    /// Windows build number via `RtlGetVersion` (not subject to the
    /// manifest-based version lie of `GetVersionEx`).
    pub(super) fn windows_build() -> Option<u32> {
        use windows_sys::Wdk::System::SystemServices::RtlGetVersion;
        use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;
        // SAFETY: OSVERSIONINFOW is plain data; RtlGetVersion fills the
        // struct whose size field we set, and always succeeds for it.
        let mut v: OSVERSIONINFOW = unsafe { std::mem::zeroed() };
        v.dwOSVersionInfoSize = std::mem::size_of::<OSVERSIONINFOW>() as u32;
        let status = unsafe { RtlGetVersion(&mut v) };
        (status == 0).then_some(v.dwBuildNumber)
    }

    /// Thread body.
    pub(super) fn run(ctx: Ctx, warn_tx: Sender<String>) {
        match windows_build() {
            Some(b) if ble_rate_limited(b) => {
                tracing::warn!(build = b, "Windows 10 BLE stack: Switch 2 Pro report rate is limited");
                let _ = warn_tx.send(WARN_BLE_RATE_LIMITED_WIN10.to_string());
            }
            Some(_) => {}
            None => tracing::warn!("could not read the Windows build number"),
        }
        let rt = match tokio::runtime::Builder::new_current_thread().enable_time().build() {
            Ok(rt) => rt,
            Err(e) => {
                tracing::error!(error = %e, "NS2 BLE: tokio runtime init failed");
                return;
            }
        };
        rt.block_on(worker(&ctx));
        tracing::info!("NS2 BLE worker stopped");
    }

    /// Adapter handle kept across sessions; re-opened after any error.
    struct Ble {
        adapter: Adapter,
        events: Events,
        /// Adapter address, MSB-first (btleplug order).
        local: [u8; 6],
    }

    async fn open_ble() -> Result<Ble> {
        let manager = Manager::new().await.context("BLE manager")?;
        let adapter = manager
            .adapters()
            .await
            .context("listing BLE adapters")?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("no Bluetooth adapter"))?;
        let addr = adapter
            .adapter_address()
            .await
            .context("adapter address")?
            .ok_or_else(|| anyhow!("adapter address unavailable"))?;
        let events = adapter.events().await.context("adapter events")?;
        tracing::info!(adapter = %addr, "NS2 BLE: scanning for a Switch 2 Pro");
        Ok(Ble { adapter, events, local: addr.into_inner() })
    }

    async fn worker(ctx: &Ctx) {
        let mut ble: Option<Ble> = None;
        // Repeated identical failures (Bluetooth off, a pad that keeps
        // dropping) are logged once, then at debug level.
        let mut last_err = String::new();
        while !ctx.stopped() {
            if ble.is_none() {
                match open_ble().await {
                    Ok(b) => ble = Some(b),
                    Err(e) => {
                        note_failure(&mut last_err, &e);
                        pause(ctx, ADAPTER_RETRY).await;
                        continue;
                    }
                }
            }
            let b = ble.as_mut().expect("opened above");
            let end = match session(ctx, b).await {
                Ok(end) => {
                    if end != SessionEnd::Failed {
                        last_err.clear();
                    }
                    end
                }
                Err(f) if ctx.stopped() => {
                    tracing::debug!(error = %format!("{:#}", f.err), "NS2 BLE: stopped mid-step");
                    SessionEnd::Stopped
                }
                Err(f) => {
                    note_failure(&mut last_err, &f.err);
                    if f.adapter {
                        ble = None;
                    }
                    SessionEnd::Failed
                }
            };
            tracing::debug!(?end, "NS2 BLE session ended");
            pause(ctx, rescan_delay(end)).await;
        }
    }

    fn note_failure(last: &mut String, e: &anyhow::Error) {
        let msg = format!("{e:#}");
        if *last != msg {
            tracing::warn!(error = %msg, "NS2 BLE: attempt failed; retrying every 2 s");
            *last = msg;
        } else {
            tracing::debug!(error = %msg, "NS2 BLE: attempt failed again");
        }
    }

    /// Sleep `d`, waking every `TICK` for the stop flag. No announced pad
    /// is streaming meanwhile, so manual disconnects are drained.
    async fn pause(ctx: &Ctx, d: Duration) {
        let end = Instant::now() + d;
        while !ctx.stopped() {
            ctx.drain_disconnects();
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            tokio::time::sleep(left.min(TICK)).await;
        }
    }

    /// Await `fut` for at most `limit`, giving up early when stopped.
    async fn bounded<F: Future>(ctx: &Ctx, limit: Duration, what: &str, fut: F) -> Result<F::Output> {
        tokio::pin!(fut);
        let end = Instant::now() + limit;
        loop {
            if ctx.stopped() {
                bail!("{what}: stopped");
            }
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                bail!("{what}: timed out after {limit:?}");
            }
            if let Ok(v) = tokio::time::timeout(left.min(TICK), &mut fut).await {
                return Ok(v);
            }
        }
    }

    /// A session that failed before its pad was announced.
    struct Fault {
        /// Scanning failed: re-open the adapter before the next attempt.
        adapter: bool,
        err: anyhow::Error,
    }

    /// One scan → connect → stream cycle.
    async fn session(ctx: &Ctx, ble: &mut Ble) -> std::result::Result<SessionEnd, Fault> {
        let scanned = scan(ctx, ble).await.map_err(|err| Fault { adapter: true, err })?;
        let Some((p, m)) = scanned else {
            return Ok(SessionEnd::Stopped);
        };
        tracing::info!(addr = %p.address(), mode = ?m, "NS2 Pro advert accepted; connecting");
        let mut input = InputStream::new(Ns2StickCal::default());
        let result = drive(ctx, ble, &p, m, &mut input).await;
        if !matches!(result, Ok(SessionEnd::TurnedOff)) {
            // Every other exit leaves the link up (or half up): drop it.
            if !matches!(tokio::time::timeout(CLEANUP_TIMEOUT, p.disconnect()).await, Ok(Ok(()))) {
                tracing::debug!("NS2 BLE: disconnect after session end failed or timed out");
            }
        }
        input.finish(&ctx.tx);
        match &result {
            Ok(end) => tracing::info!(?end, connected = input.announced(), "NS2 Pro session ended"),
            Err(e) if input.announced() => {
                tracing::warn!(error = %format!("{e:#}"), "NS2 Pro link failed")
            }
            Err(_) => {}
        }
        // A pad that streamed and then failed is a lost link, not a
        // connect failure.
        match result {
            Ok(end) => Ok(end),
            Err(_) if input.announced() => Ok(SessionEnd::LinkLost),
            Err(err) => Err(Fault { adapter: false, err }),
        }
    }

    /// Scan until an acceptable advert. `None` when stopped.
    async fn scan(ctx: &Ctx, ble: &mut Ble) -> Result<Option<(Peripheral, AdvertMatch)>> {
        ble.adapter.start_scan(ScanFilter::default()).await.context("start_scan")?;
        let found = scan_until_match(ctx, ble).await;
        let _ = ble.adapter.stop_scan().await;
        found
    }

    async fn scan_until_match(ctx: &Ctx, ble: &mut Ble) -> Result<Option<(Peripheral, AdvertMatch)>> {
        loop {
            // Checked every iteration: adverts from nearby devices can
            // arrive faster than `TICK`.
            if ctx.stopped() {
                return Ok(None);
            }
            ctx.drain_disconnects();
            let ev = match tokio::time::timeout(TICK, ble.events.next()).await {
                Err(_) => continue,
                Ok(None) => bail!("adapter event stream closed"),
                Ok(Some(ev)) => ev,
            };
            if let CentralEvent::ManufacturerDataAdvertisement { id, manufacturer_data } = ev {
                if let Some(m) = accept_advert(&manufacturer_data, ble.local) {
                    let p = ble.adapter.peripheral(&id).await.context("peripheral lookup")?;
                    return Ok(Some((p, m)));
                }
            }
        }
    }

    struct Link<'a> {
        p: &'a Peripheral,
        cmd: Characteristic,
        input: Characteristic,
        notes: Notes,
    }

    impl Link<'_> {
        /// Write a command (without response) and wait for the response
        /// notification `done` accepts. Other notifications are skipped.
        async fn request(
            &mut self,
            ctx: &Ctx,
            pkt: &[u8],
            done: impl Fn(&[u8]) -> bool,
        ) -> Result<Vec<u8>> {
            let (cmd, sub) = (pkt[0], pkt[3]);
            self.send(ctx, pkt).await?;
            let end = Instant::now() + RESPONSE_TIMEOUT;
            loop {
                if ctx.stopped() {
                    bail!("command {cmd:#04x}/{sub:#04x}: stopped");
                }
                let left = end.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    bail!("no response to command {cmd:#04x}/{sub:#04x}");
                }
                match tokio::time::timeout(left.min(TICK), self.notes.next()).await {
                    Ok(Some(n)) if n.uuid.as_u128() == UUID_RESP && done(&n.value) => {
                        return Ok(n.value)
                    }
                    Ok(Some(_)) | Err(_) => {}
                    Ok(None) => bail!("notification stream closed"),
                }
            }
        }

        async fn send(&self, ctx: &Ctx, pkt: &[u8]) -> Result<()> {
            let write = self.p.write(&self.cmd, pkt, WriteType::WithoutResponse);
            bounded(ctx, GATT_OP_TIMEOUT, "command write", write).await?.context("command write")
        }
    }

    /// Connect, initialize, stream. Returns how the session ended.
    async fn drive(
        ctx: &Ctx,
        ble: &mut Ble,
        p: &Peripheral,
        m: AdvertMatch,
        input: &mut InputStream,
    ) -> Result<SessionEnd> {
        // ── Connecting (order is hardware-proven, spec §12) ──
        bounded(ctx, CONNECT_TIMEOUT, "connect", p.connect()).await?.context("connect")?;
        let preset = ConnectionParameterPreset::ThroughputOptimized;
        if let Err(e) = p.request_connection_parameters(preset).await {
            tracing::warn!(error = %e, "NS2 BLE: connection-parameter request failed");
        }
        pause(ctx, PARAMS_SETTLE).await;
        bounded(ctx, DISCOVER_TIMEOUT, "discover_services", p.discover_services())
            .await?
            .context("discover_services")?;
        let chars = p.characteristics();
        let find = |u: u128| {
            chars
                .iter()
                .find(|c| c.uuid.as_u128() == u)
                .cloned()
                .ok_or_else(|| anyhow!("characteristic {u:032x} missing"))
        };
        let (cmd, input_char, resp) = (find(UUID_CMD)?, find(UUID_INPUT)?, find(UUID_RESP)?);
        // Take the notification stream before enabling notify so no
        // response is missed.
        let notes = p.notifications().await.context("notification stream")?;
        let mut link = Link { p, cmd, input: input_char, notes };
        bounded(ctx, GATT_OP_TIMEOUT, "subscribe response", p.subscribe(&resp))
            .await?
            .context("subscribe response")?;

        // ── Initializing ──
        if m == AdvertMatch::PairingMode {
            pair(ctx, &mut link, ble.local).await?;
        }
        input.set_cal(read_stick_cal(ctx, &mut link).await);
        let led = command(CMD_LED, SUB_LED_SET, &LED_PLAYER_1);
        if let Err(e) = link.request(ctx, &led, |r| is_response_to(r, CMD_LED, SUB_LED_SET)).await {
            if ctx.stopped() {
                return Err(e);
            }
            tracing::warn!(error = %format!("{e:#}"), "NS2 BLE: player LED not acknowledged");
        }
        bounded(ctx, GATT_OP_TIMEOUT, "subscribe input", p.subscribe(&link.input))
            .await?
            .context("subscribe input")?;

        // ── Streaming ──
        // A request made before this pad was announced is not for it.
        ctx.drain_disconnects();
        input.arm(Instant::now());
        stream(ctx, ble, &mut link, input).await
    }

    async fn stream(
        ctx: &Ctx,
        ble: &mut Ble,
        link: &mut Link<'_>,
        input: &mut InputStream,
    ) -> Result<SessionEnd> {
        let id = link.p.id();
        loop {
            // Checked every iteration: reports arrive every ~15 ms.
            if ctx.stopped() {
                return Ok(SessionEnd::Stopped);
            }
            if ctx.disconnect_rx.try_recv().is_ok() && input.announced() {
                turn_off(ctx, link).await;
                return Ok(SessionEnd::TurnedOff);
            }
            let now = Instant::now();
            if input.expired(now) {
                tracing::info!("NS2 Pro: no report for {WATCHDOG_LIMIT:?}");
                return Ok(SessionEnd::LinkLost);
            }
            let wait = input.remaining(now).unwrap_or(TICK).min(TICK);
            tokio::select! {
                n = link.notes.next() => match n {
                    Some(n) if n.uuid.as_u128() == UUID_INPUT => {
                        input.on_report(&n.value, Instant::now(), &ctx.tx);
                    }
                    Some(_) => {}
                    None => bail!("notification stream closed"),
                },
                ev = ble.events.next() => match ev {
                    Some(CentralEvent::DeviceDisconnected(d)) if d == id => {
                        tracing::info!("NS2 Pro: device disconnected");
                        return Ok(SessionEnd::LinkLost);
                    }
                    Some(_) => {}
                    None => bail!("adapter event stream closed"),
                },
                _ = tokio::time::sleep(wait) => {}
            }
        }
    }

    /// Manual disconnect: command 0x06/0x02 (no response expected), then
    /// drop the BLE link. Best effort; the link goes down either way.
    async fn turn_off(ctx: &Ctx, link: &Link<'_>) {
        tracing::info!("NS2 Pro: manual disconnect — turning the pad off");
        let off = command(CMD_POWER, SUB_POWER_OFF, &[0u8; 12]);
        if let Err(e) = link.send(ctx, &off).await {
            tracing::warn!(error = %format!("{e:#}"), "NS2 BLE: turn-off command failed");
        } else {
            pause(ctx, TURN_OFF_SETTLE).await;
        }
        match tokio::time::timeout(CLEANUP_TIMEOUT, link.p.disconnect()).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::warn!(error = %e, "NS2 BLE: disconnect failed"),
            Err(_) => tracing::warn!("NS2 BLE: disconnect timed out"),
        }
    }

    /// The `0x15` pairing (pairing-mode adverts only). A B2 that does not
    /// match our computation is logged and the pairing still committed.
    async fn pair(ctx: &Ctx, link: &mut Link<'_>, local: [u8; 6]) -> Result<()> {
        tracing::info!("NS2 Pro in pairing mode: pairing with this adapter");
        let addr = command(CMD_PAIR, SUB_PAIR_ADDR, &pairing_addresses(local));
        link.request(ctx, &addr, |r| is_response_to(r, CMD_PAIR, SUB_PAIR_ADDR)).await?;
        let keys = PairingKeys::random();
        let a1 = pairing_key_cmd(SUB_PAIR_A1, &keys.a1);
        let r = link.request(ctx, &a1, |r| is_response_to(r, CMD_PAIR, SUB_PAIR_A1)).await?;
        let b1 = pairing_response_key(&r, SUB_PAIR_A1);
        let a2 = pairing_key_cmd(SUB_PAIR_A2, &keys.a2);
        let r = link.request(ctx, &a2, |r| is_response_to(r, CMD_PAIR, SUB_PAIR_A2)).await?;
        let b2 = pairing_response_key(&r, SUB_PAIR_A2);
        match (b1, b2) {
            (Some(b1), Some(b2)) if expected_b2(&ltk(&keys.a1, &b1), &keys.a2) == b2 => {
                tracing::info!("NS2 Pro pairing: B2 verified")
            }
            _ => tracing::warn!("NS2 Pro pairing: B2 mismatch or missing; committing anyway"),
        }
        let commit = command(CMD_PAIR, SUB_PAIR_COMMIT, &[0x00]);
        link.request(ctx, &commit, |r| is_response_to(r, CMD_PAIR, SUB_PAIR_COMMIT)).await?;
        tracing::info!("NS2 Pro pairing committed");
        Ok(())
    }

    /// Factory stick calibration. A missing or malformed block keeps the
    /// defaults (centre 2048, span 1400) for that stick.
    async fn read_stick_cal(ctx: &Ctx, link: &mut Link<'_>) -> Ns2StickCal {
        let mut blocks: [Vec<u8>; 2] = [Vec::new(), Vec::new()];
        for (i, addr) in [SPI_LEFT_STICK_CAL, SPI_RIGHT_STICK_CAL].into_iter().enumerate() {
            let pkt = spi_read_cmd(addr, SPI_CAL_LEN);
            match link.request(ctx, &pkt, |r| spi_read_payload(r, addr).is_some()).await {
                Ok(r) => {
                    blocks[i] = spi_read_payload(&r, addr).map(<[u8]>::to_vec).unwrap_or_default()
                }
                Err(_) if ctx.stopped() => break,
                Err(e) => tracing::warn!(
                    addr = format_args!("{addr:#x}"),
                    error = %format!("{e:#}"),
                    "NS2 Pro: stick calibration unreadable; using defaults"
                ),
            }
        }
        parse_stick_cal(&blocks[0], &blocks[1])
    }
}

// ─── Other platforms: stub ─────────────────────────────────────────────────

/// Non-Windows stand-in: never connects, never warns. Manual disconnect
/// signals are drained so they do not pile up.
#[cfg(not(windows))]
pub struct Ns2BleSource {
    disconnect_rx: Receiver<()>,
}

#[cfg(not(windows))]
impl Ns2BleSource {
    pub fn new(disconnect_rx: Receiver<()>, _warn_tx: Sender<String>) -> anyhow::Result<Self> {
        Ok(Self { disconnect_rx })
    }

    pub fn poll(&mut self, _out: &mut Vec<GamepadEvent>) {
        while self.disconnect_rx.try_recv().is_ok() {}
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::gamepad::events::GamepadEvent;
    use crate::gamepad::ns2_protocol::{AdvertMatch, Ns2StickCal, NS2_COMPANY_ID};
    use crate::gamepad::ControllerKind;
    use crossbeam_channel::unbounded;
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    fn hex(s: &str) -> Vec<u8> {
        s.split_whitespace().map(|b| u8::from_str_radix(b, 16).unwrap()).collect()
    }

    /// Local adapter of the spike runs (34:FD:70:A3:21:85).
    const LOCAL: [u8; 6] = [0x34, 0xFD, 0x70, 0xA3, 0x21, 0x85];
    const ADV_PAIRING: &str = "01 00 03 7e 05 69 20 00 01 00 00 00 00 00 00 00 0f 00 00 00 00 00 00 00";
    const ADV_BONDED: &str = "01 00 03 7e 05 69 20 00 01 00 85 21 a3 70 fd 34 0f 00 00 00 00 00 00 00";
    const R2_FIRST_REPORT: &str = "69 0d 00 00 00 00 00 00 00 00 ef 27 86 9d 88 7d 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 1d 0e 00 00 00 00 00 00 00 00 01 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00";

    fn mfr(company: u16, data: &str) -> HashMap<u16, Vec<u8>> {
        HashMap::from([(company, hex(data))])
    }

    #[test]
    fn advert_accepts_pairing_mode_and_bonded_to_us() {
        assert_eq!(
            accept_advert(&mfr(NS2_COMPANY_ID, ADV_PAIRING), LOCAL),
            Some(AdvertMatch::PairingMode)
        );
        assert_eq!(accept_advert(&mfr(NS2_COMPANY_ID, ADV_BONDED), LOCAL), Some(AdvertMatch::Bonded));
        // Other manufacturers' entries alongside ours do not matter.
        let mut both = mfr(NS2_COMPANY_ID, ADV_BONDED);
        both.insert(0x004C, vec![0x02, 0x15]);
        assert_eq!(accept_advert(&both, LOCAL), Some(AdvertMatch::Bonded));
    }

    #[test]
    fn advert_rejects_everything_else() {
        // Bonded to another host.
        let mut other = LOCAL;
        other[5] = 0x86;
        assert_eq!(accept_advert(&mfr(NS2_COMPANY_ID, ADV_BONDED), other), None);
        // Same bytes under another company id.
        assert_eq!(accept_advert(&mfr(0x004C, ADV_PAIRING), LOCAL), None);
        // No manufacturer data at all.
        assert_eq!(accept_advert(&HashMap::new(), LOCAL), None);
        // Truncated before the host field.
        let short = hex(ADV_PAIRING)[..15].to_vec();
        assert_eq!(accept_advert(&HashMap::from([(NS2_COMPANY_ID, short)]), LOCAL), None);
        // Joy-Con 2 R (PID 0x2067) in pairing mode.
        let mut jc = hex(ADV_PAIRING);
        jc[5] = 0x67;
        assert_eq!(accept_advert(&HashMap::from([(NS2_COMPANY_ID, jc)]), LOCAL), None);
    }

    #[test]
    fn watchdog_expires_at_limit_and_feed_resets() {
        let t0 = Instant::now();
        let mut w = Watchdog::new(t0, WATCHDOG_LIMIT);
        assert_eq!(WATCHDOG_LIMIT, Duration::from_millis(1000));
        assert!(!w.expired(t0));
        assert!(!w.expired(t0 + Duration::from_millis(999)));
        assert!(w.expired(t0 + Duration::from_millis(1000)));
        assert_eq!(w.remaining(t0 + Duration::from_millis(400)), Duration::from_millis(600));
        assert_eq!(w.remaining(t0 + Duration::from_millis(1500)), Duration::ZERO);
        w.feed(t0 + Duration::from_millis(900));
        assert!(!w.expired(t0 + Duration::from_millis(1800)));
        assert!(w.expired(t0 + Duration::from_millis(1900)));
    }

    #[test]
    fn rescan_delays() {
        assert_eq!(rescan_delay(SessionEnd::Failed), Duration::from_secs(2));
        assert_eq!(rescan_delay(SessionEnd::LinkLost), Duration::from_secs(2));
        // A turned-off pad must not be re-grabbed while it goes to sleep.
        assert_eq!(rescan_delay(SessionEnd::TurnedOff), Duration::from_secs(3));
        assert_eq!(rescan_delay(SessionEnd::Stopped), Duration::ZERO);
    }

    #[test]
    fn win10_builds_are_rate_limited() {
        assert!(ble_rate_limited(19045)); // Windows 10 22H2
        assert!(ble_rate_limited(21999));
        assert!(!ble_rate_limited(22000)); // Windows 11 21H2
        assert!(!ble_rate_limited(26200)); // the spike machine
    }

    #[test]
    fn input_stream_connects_on_first_decoded_report() {
        let (tx, rx) = unbounded();
        let t0 = Instant::now();
        let mut s = InputStream::new(Ns2StickCal::default());
        s.arm(t0);
        assert!(!s.announced());

        let mut report = hex(R2_FIRST_REPORT);
        assert!(s.on_report(&report, t0 + Duration::from_millis(15), &tx));
        let evs: Vec<GamepadEvent> = rx.try_iter().collect();
        assert_eq!(evs[0], GamepadEvent::Connected(ControllerKind::Ns2Pro));
        assert_eq!(evs.iter().filter(|e| matches!(e, GamepadEvent::Connected(_))).count(), 1);
        // The first frame carries the full state (4 sticks + 2 triggers).
        assert_eq!(evs.len(), 1 + 6);
        assert!(s.announced());

        // B pressed: only the diff, no second Connected.
        report[4] = 0x04;
        assert!(s.on_report(&report, t0 + Duration::from_millis(30), &tx));
        let evs: Vec<GamepadEvent> = rx.try_iter().collect();
        assert_eq!(evs, vec![GamepadEvent::ButtonDown(0)]);

        // The watchdog runs from the last decoded report.
        assert!(!s.expired(t0 + Duration::from_millis(1029)));
        assert!(s.expired(t0 + Duration::from_millis(1030)));
    }

    #[test]
    fn input_stream_ignores_undecodable_reports() {
        let (tx, rx) = unbounded();
        let t0 = Instant::now();
        let mut s = InputStream::new(Ns2StickCal::default());
        s.arm(t0);
        assert!(!s.on_report(&[0u8; 15], t0 + Duration::from_millis(500), &tx));
        assert_eq!(rx.try_iter().count(), 0, "no Connected without a decoded report");
        assert!(!s.announced());
        // An undecodable report does not feed the watchdog.
        assert!(s.expired(t0 + Duration::from_millis(1000)));
    }

    #[test]
    fn input_stream_disconnected_only_after_connected() {
        let (tx, rx) = unbounded();
        let t0 = Instant::now();
        let s = InputStream::new(Ns2StickCal::default());
        s.finish(&tx);
        assert_eq!(rx.try_iter().count(), 0, "never connected → no Disconnected");

        let mut s = InputStream::new(Ns2StickCal::default());
        s.arm(t0);
        s.on_report(&hex(R2_FIRST_REPORT), t0, &tx);
        let _ = rx.try_iter().count();
        s.finish(&tx);
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), vec![GamepadEvent::Disconnected]);
    }

    #[test]
    #[cfg(not(windows))]
    fn stub_never_connects() {
        let (dtx, drx) = unbounded::<()>();
        let (wtx, wrx) = unbounded::<String>();
        let mut src = Ns2BleSource::new(drx, wtx).unwrap();
        let _ = dtx.send(());
        let mut out = Vec::new();
        for _ in 0..5 {
            src.poll(&mut out);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(out.is_empty(), "stub emitted {out:?}");
        assert!(wrx.try_recv().is_err(), "stub sends no warnings");
        assert!(dtx.is_empty(), "stub drains manual disconnect signals");
    }

    #[test]
    #[cfg(windows)]
    fn windows_build_is_known() {
        let b = imp::windows_build().expect("RtlGetVersion");
        assert!(b >= 10240, "build {b}");
    }
}
