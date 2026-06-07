//! DualSense BT raw HID source.
//!
//! Spawns a worker thread that owns the hidapi `HidDevice`, drives the
//! `Searching → Handshaking → Streaming` state machine, decodes 78-byte
//! 0x31 reports, diffs against the previous snapshot, and pushes
//! `GamepadEvent`s to a channel the engine drains via `poll()`.
//!
//! Three constructors:
//! - `new()`             — production hidapi worker
//! - `fake(rx)`          — direct GamepadEvent injection (engine tests)
//! - `new_from_byte_stream(rx)` — raw 78-byte injection (state-machine tests)

use super::cursor_params::CursorParams;
use super::ds_protocol::{build_handshake_buffer, decode_31, DsState, REPORT_LEN_BT};
use super::events::GamepadEvent;
use anyhow::{anyhow, Context, Result};
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Per-frame raw-coordinate delta upper bound. A real finger at 250 Hz
/// frame rate over the 1920-wide pad cannot move more than ~150 raw px
/// in one frame. Larger jumps come from the DualSense reporting one
/// stale (pre-touch) frame at touch-down — the very first contact
/// frame can carry the x/y of the previous touch even though `active`
/// has flipped to true. We re-anchor on these without emitting so the
/// cursor stays still rather than teleporting halfway across the screen.
const CURSOR_TELEPORT_GUARD: i32 = 250;
/// Button ids 25..=28 = Touchpad TL/TR/BL/BR. Matches
/// `config::TOUCHPAD_QUADRANT_IDS`; duplicated here to avoid pulling
/// the config crate into the gamepad layer.
const QUAD_TL: u32 = 25;
const QUAD_TR: u32 = 26;
const QUAD_BL: u32 = 27;
const QUAD_BR: u32 = 28;
/// Sentinel value emitted in `GamepadEvent::TouchpadHover.quadrant`
/// when the finger lifts. Out of band — valid quadrant ids are 25..=28.
const HOVER_QUADRANT_NONE: u32 = 255;

/// Bug 1: time window after click rising edge during which cursor
/// delta is suppressed to mask finger-press lateral roll. Matches
/// libinput tap timeout order of magnitude (~100ms). Tuned conservatively
/// short so press-and-drag becomes responsive after the press settles.
const CLICK_FREEZE_MS: u64 = 80;
/// Bug 1: cumulative raw-pixel motion threshold from click anchor.
/// If finger moves > this from anchor during the freeze window, user
/// intent is dragging — exit freeze and let cursor follow. 15 raw px
/// ≈ 1% of touchpad width, well above press-roll drift (5-10 px).
const CLICK_DRAG_EXIT_PX: u32 = 15;
/// Bug 2: rolling position buffer depth (frames). 4ms/frame × 10 = 40ms,
/// enough to reach back to a stable pre-press position (~30ms target).
const POS_BUFFER_DEPTH: usize = 10;
/// Bug 2: target lookback for click quadrant pos — pick the buffer entry
/// closest to (now - this) when the click rising edge fires.
const CLICK_POS_LOOKBACK_MS: u64 = 30;

/// Per-worker touchpad tracking. Reset on each new device connect.
#[derive(Default)]
struct TouchpadState {
    last_finger_pos: Option<(u16, u16)>,
    /// Position recorded at the moment finger 0 first became active in
    /// the current contact. The quadrant for the touchpad click uses
    /// this — the user's *intent* is where they put their finger, not
    /// where the finger has drifted to by the time the physical click
    /// switch fires (the press motion shifts the contact point).
    touchdown_pos: Option<(u16, u16)>,
    last_click_quadrant: Option<u32>,
    prev_touchpad_btn: bool,
    /// Mirrors current frame's touchpad button state (buf[11] bit 1).
    /// Updated every frame in `process_touchpad`. Read by the cursor
    /// filter so L1 (click-freeze) can suppress cursor deltas while
    /// the button is physically held.
    click_btn_held: bool,
    /// Rolling buffer of last 3 frames' squared motion magnitudes,
    /// used by L2 (stationary deadzone). Populated by the filter; the
    /// state outlives a single frame so consecutive sub-radius motion
    /// is suppressed only after the radius has been crossed back.
    recent_mag_sq: std::collections::VecDeque<u32>,
    /// Last quadrant the finger 0 was over (None if not currently in
    /// any quadrant — i.e., finger lifted). Used to dedupe per-frame
    /// hover emits so only quadrant CHANGES produce events.
    last_hover_quadrant: Option<u32>,
    /// Time the touchpad button transitioned false→true (Bug 1: click drift).
    /// `None` = button currently up. Set on rising edge.
    click_btn_down_at: Option<Instant>,
    /// Finger position captured at click rising edge (Bug 1).
    /// Cumulative motion from this anchor gates the L1 freeze early-exit.
    click_anchor_pos: Option<(u16, u16)>,
    /// Rolling buffer of recent (x, y, timestamp) for stable pre-press
    /// position lookup. Bug 2: click quadrant uses entry from ~30ms ago,
    /// not the live finger position (which has press-drift) and not
    /// `touchdown_pos` (which is stale across multi-click drag sessions).
    pos_buffer: std::collections::VecDeque<(u16, u16, Instant)>,
}

fn quadrant_for(x: u16, y: u16, mid_x: u16, mid_y: u16) -> u32 {
    match (x < mid_x, y < mid_y) {
        (true,  true)  => QUAD_TL,
        (false, true)  => QUAD_TR,
        (true,  false) => QUAD_BL,
        (false, false) => QUAD_BR,
    }
}

/// Per-frame hover quadrant emit with dedupe-on-change.
///
/// While the finger is active, emits `TouchpadHover` only when the
/// quadrant computed from the current (raw_x, raw_y) differs from the
/// last emitted quadrant. On finger lift, emits a sentinel
/// `TouchpadHover { quadrant: 255, raw_x: 0, raw_y: 0 }` to signal
/// "clear hover".
///
/// Uses the same `quadrant_for` axis-rect logic as click handling so
/// hover preview and click outcome are guaranteed consistent.
fn process_touchpad_hover<F: FnMut(GamepadEvent)>(
    finger_active: bool,
    cur_x: u16,
    cur_y: u16,
    state: &mut TouchpadState,
    params: &CursorParams,
    mut emit: F,
) {
    if finger_active {
        let q = quadrant_for(cur_x, cur_y, params.midpoint_x(), params.midpoint_y());
        if state.last_hover_quadrant != Some(q) {
            state.last_hover_quadrant = Some(q);
            emit(GamepadEvent::TouchpadHover { raw_x: cur_x, raw_y: cur_y, quadrant: q });
        }
    } else if state.last_hover_quadrant.is_some() {
        state.last_hover_quadrant = None;
        emit(GamepadEvent::TouchpadHover { raw_x: 0, raw_y: 0, quadrant: HOVER_QUADRANT_NONE });
    }
}

/// Three-layer cursor delta filter for the DualSense touchpad. Each
/// frame's raw motion is passed through:
///   L1 — Click freeze: while the user is physically holding the
///        touchpad button, suppress all cursor motion. Matches the
///        Synaptics PalmCheck / libinput thumb-detect behaviour for
///        Clickpads. Avoids the 5–10 px lateral drift caused by the
///        finger rolling forward as the user presses the button.
///   L2 — Stationary deadzone (added in Task 9).
///   L3 — Acceleration curve     (added in Task 10).
///
/// Returns the filtered (dx, dy) to emit, or `None` to suppress.
pub(crate) fn filter_cursor_delta(
    raw_dx: i32,
    raw_dy: i32,
    state: &mut TouchpadState,
    params: &CursorParams,
) -> Option<(i32, i32)> {
    // L1: motion-gated click freeze. Mask press-roll lateral drift in
    // the first CLICK_FREEZE_MS window OR until cumulative motion from
    // the click anchor exceeds CLICK_DRAG_EXIT_PX (drag detected).
    // After either gate exits, cursor follows finger normally so
    // press-and-drag works.
    if state.click_btn_held
        && params.click_freeze_enabled()
        && state.click_btn_down_at.is_some()
        && state.click_anchor_pos.is_some()
    {
        let down_at = state.click_btn_down_at.unwrap();
        let (anchor_x, anchor_y) = state.click_anchor_pos.unwrap();
        let elapsed_ms = down_at.elapsed().as_millis() as u64;
        // Cumulative motion from anchor — computed via the pos_buffer's
        // latest entry (updated BEFORE filter_cursor_delta is called).
        let cur_pos = state.pos_buffer.back().map(|(x, y, _)| (*x, *y));
        let cum_mag_sq = if let Some((cx, cy)) = cur_pos {
            let ddx = (cx as i32) - (anchor_x as i32);
            let ddy = (cy as i32) - (anchor_y as i32);
            (ddx * ddx + ddy * ddy) as u32
        } else {
            0
        };
        let exit_sq = CLICK_DRAG_EXIT_PX * CLICK_DRAG_EXIT_PX;
        if elapsed_ms < CLICK_FREEZE_MS && cum_mag_sq < exit_sq {
            return None;
        }
    }
    // L2: stationary deadzone — rolling 3-frame magnitude window
    let mag_sq = (raw_dx * raw_dx + raw_dy * raw_dy) as u32;
    state.recent_mag_sq.push_back(mag_sq);
    if state.recent_mag_sq.len() > 3 {
        state.recent_mag_sq.pop_front();
    }
    let dz = params.deadzone_radius();
    let dz_sq = (dz * dz) as u32;
    if state.recent_mag_sq.iter().all(|m| *m < dz_sq) {
        return None;
    }
    // L3: acceleration curve — linear interp gain between slow / fast thresholds
    let mag_px = (mag_sq as f32).sqrt();
    let slow = params.accel_slow_threshold() as f32;
    let fast = params.accel_fast_threshold() as f32;
    let g_slow = params.accel_gain_slow();
    let g_fast = params.accel_gain_fast();
    let gain = if mag_px < slow {
        g_slow
    } else if mag_px > fast {
        g_fast
    } else {
        let t = (mag_px - slow) / (fast - slow);
        g_slow + t * (g_fast - g_slow)
    };
    let sens = params.sensitivity();
    let total = sens * gain;
    Some(((raw_dx as f32 * total) as i32, (raw_dy as f32 * total) as i32))
}

/// Push current finger position into the rolling buffer; pop the oldest
/// entry if at depth. Called every frame in `process_touchpad`.
fn record_finger_pos(state: &mut TouchpadState, x: u16, y: u16, now: Instant) {
    state.pos_buffer.push_back((x, y, now));
    if state.pos_buffer.len() > POS_BUFFER_DEPTH {
        state.pos_buffer.pop_front();
    }
}

/// Look up the buffered position closest to `target = now - lookback`.
/// Returns the entry whose timestamp is nearest the target. Falls back
/// to the oldest available entry if the buffer is shorter than lookback.
fn stable_pos_for_click(
    state: &TouchpadState,
    now: Instant,
    lookback: Duration,
) -> Option<(u16, u16)> {
    let target = now.checked_sub(lookback)?;
    let mut best: Option<&(u16, u16, Instant)> = None;
    let mut best_diff_ms = u128::MAX;
    for entry in state.pos_buffer.iter() {
        let diff_ms = if entry.2 > target {
            (entry.2 - target).as_millis()
        } else {
            (target - entry.2).as_millis()
        };
        if diff_ms < best_diff_ms {
            best_diff_ms = diff_ms;
            best = Some(entry);
        }
    }
    best.map(|(x, y, _)| (*x, *y))
}

/// Cursor delta + touchpad click → 4-quadrant button events. Mutates
/// `state` across frames. Called per decoded `DsState`.
fn process_touchpad(
    state: &mut TouchpadState,
    cur: &DsState,
    params: &CursorParams,
    tx: &Sender<GamepadEvent>,
) {
    let now = Instant::now();
    // Update rolling position buffer before any filter/click logic so
    // filter_cursor_delta and stable_pos_for_click see the latest data.
    if cur.finger0_active {
        record_finger_pos(state, cur.finger0_x, cur.finger0_y, now);
    } else {
        state.pos_buffer.clear();
    }

    // Click button state for L1 freeze — set every frame so the filter
    // sees the current button state, not the previous frame's.
    state.click_btn_held = cur.touchpad_btn;
    // Cursor: relative motion from the previous finger 0 position.
    if cur.finger0_active {
        match state.last_finger_pos {
            None => {
                // Touch-down — record the start position without emitting.
                state.last_finger_pos = Some((cur.finger0_x, cur.finger0_y));
                state.touchdown_pos = Some((cur.finger0_x, cur.finger0_y));
            }
            Some((lx, ly)) => {
                let dx_raw = cur.finger0_x as i32 - lx as i32;
                let dy_raw = cur.finger0_y as i32 - ly as i32;
                if dx_raw.abs() > CURSOR_TELEPORT_GUARD
                    || dy_raw.abs() > CURSOR_TELEPORT_GUARD
                {
                    // Stale touch-down frame — re-anchor silently. The
                    // previous touchdown was garbage; replace it with the
                    // new real coordinates so the next click sees the
                    // settled position.
                    state.last_finger_pos = Some((cur.finger0_x, cur.finger0_y));
                    state.touchdown_pos = Some((cur.finger0_x, cur.finger0_y));
                } else {
                    // Always advance the anchor so deltas don't accumulate stale,
                    // but only emit if the 3-layer filter (freeze/deadzone/curve)
                    // approves the frame. Filter handles sensitivity scaling too.
                    state.last_finger_pos = Some((cur.finger0_x, cur.finger0_y));
                    // touchdown_pos intentionally NOT updated on motion — it
                    // represents the user's intent at touch-down.
                    if params.enabled() {
                        if let Some((dx, dy)) = filter_cursor_delta(dx_raw, dy_raw, state, params) {
                            let _ = tx.send(GamepadEvent::MouseDelta { dx, dy });
                        }
                    }
                }
            }
        }
    } else {
        // Finger lifted — clear references so the next touch-down
        // doesn't synthesise a jump from the old position.
        state.last_finger_pos = None;
        state.touchdown_pos = None;
    }

    // Hover preview — emit per-frame quadrant change for live UI feedback.
    process_touchpad_hover(
        cur.finger0_active,
        cur.finger0_x,
        cur.finger0_y,
        state,
        params,
        |ev| { let _ = tx.send(ev); },
    );

    // Click: rising edge captures the quadrant using a stable pre-press
    // position (~30ms ago) to avoid press-drift and stale touchdown_pos.
    // Falling edge releases the same id (so a drag across quadrant
    // boundaries does not re-emit).
    if cur.touchpad_btn && !state.prev_touchpad_btn {
        // Bug 1: record press timestamp + anchor for the motion-gate freeze.
        state.click_btn_down_at = Some(now);
        state.click_anchor_pos = if cur.finger0_active {
            Some((cur.finger0_x, cur.finger0_y))
        } else {
            None
        };
        // Bug 2: click quadrant uses stable position from ~30ms ago,
        // NOT touchdown_pos (which is stale across multi-click drags).
        let stable = stable_pos_for_click(
            state,
            now,
            Duration::from_millis(CLICK_POS_LOOKBACK_MS),
        );
        let click_pos = stable
            .or(state.click_anchor_pos)
            .or(state.touchdown_pos)
            .or_else(|| if cur.finger0_active {
                Some((cur.finger0_x, cur.finger0_y))
            } else {
                None
            });
        let mid_x = params.midpoint_x();
        let mid_y = params.midpoint_y();
        let (raw_x, raw_y) = click_pos.unwrap_or((mid_x, mid_y));
        let q = quadrant_for(raw_x, raw_y, mid_x, mid_y);
        tracing::info!(
            x = raw_x,
            y = raw_y,
            mid_x,
            mid_y,
            quadrant = q,
            "touchpad click captured (stable pre-press)"
        );
        state.last_click_quadrant = Some(q);
        let _ = tx.send(GamepadEvent::TouchpadClick { raw_x, raw_y, quadrant: q });
        let _ = tx.send(GamepadEvent::ButtonDown(q));
    } else if !cur.touchpad_btn && state.prev_touchpad_btn {
        // Click falling edge — clear freeze state.
        state.click_btn_down_at = None;
        state.click_anchor_pos = None;
        if let Some(q) = state.last_click_quadrant.take() {
            let _ = tx.send(GamepadEvent::ButtonUp(q));
        }
    }
    state.prev_touchpad_btn = cur.touchpad_btn;
}

/// Sony Computer Entertainment.
const DS_VID: u16 = 0x054c;
/// DualSense (standard PS5 pad).
const DS_PID: u16 = 0x0ce6;

/// Sleep between enumeration attempts in Searching state.
const ENUM_RETRY_INTERVAL: Duration = Duration::from_millis(1000);

/// Blocking read timeout per HID frame. 4 ms matches BT 250 Hz cadence.
const READ_TIMEOUT_MS: i32 = 4;
/// After this many consecutive read-timeouts (each `READ_TIMEOUT_MS`),
/// declare the pad disconnected. 50 × 4 ms = 200 ms.
const DISCONNECT_AFTER_TIMEOUTS: u32 = 50;

pub struct HidSource {
    rx: Receiver<GamepadEvent>,
    stop: Arc<AtomicBool>,
}

impl HidSource {
    /// Spawn the production hidapi worker. The worker thread starts in
    /// `Searching` and emits no events until the first 0x31 frame is
    /// decoded. `params` carries the live touchpad cursor sensitivity
    /// and on/off flag; the engine mutates them through `set_settings`.
    /// `disconnect_rx` receives manual disconnect signals from the GUI
    /// (Task 13 wires the actual state transition; ignored here with `_` prefix).
    pub fn new(params: CursorParams, disconnect_rx: crossbeam_channel::Receiver<()>) -> Result<Self> {
        let (tx, rx) = unbounded::<GamepadEvent>();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = stop.clone();
        thread::Builder::new()
            .name("dualsense-hid".into())
            .spawn(move || worker_real(tx, stop_for_thread, params, disconnect_rx))
            .map_err(|e| anyhow!("spawning HID worker thread: {e}"))?;
        Ok(Self { rx, stop })
    }

    /// Test-only constructor for engine integration tests. Drains
    /// `GamepadEvent`s directly from the supplied channel — no HID
    /// pipeline, no state machine.
    #[doc(hidden)]
    pub fn fake(rx: Receiver<GamepadEvent>) -> Self {
        Self { rx, stop: Arc::new(AtomicBool::new(false)) }
    }

    /// Test-only constructor for state-machine tests. Decodes
    /// `Vec<u8>` frames as if they came from hidapi and runs the full
    /// diff pipeline. Uses default cursor params (1.5, enabled=true).
    #[doc(hidden)]
    pub fn new_from_byte_stream(byte_rx: Receiver<Vec<u8>>) -> Self {
        Self::new_from_byte_stream_with_params(byte_rx, CursorParams::default())
    }

    /// Same as `new_from_byte_stream` but with explicit cursor params.
    #[doc(hidden)]
    pub fn new_from_byte_stream_with_params(
        byte_rx: Receiver<Vec<u8>>,
        params: CursorParams,
    ) -> Self {
        let (tx, rx) = unbounded::<GamepadEvent>();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = stop.clone();
        let (dummy_tx, dummy_rx) = crossbeam_channel::unbounded::<()>();
        thread::Builder::new()
            .name("dualsense-hid-fake".into())
            // Move dummy_tx into the closure so the disconnect channel's sender
            // stays alive for the lifetime of the worker thread. Without this,
            // the sender would be dropped immediately and the closed channel
            // would be selected on every iteration.
            .spawn(move || {
                let _keep_alive = dummy_tx;
                worker_byte_stream(byte_rx, tx, stop_for_thread, params, dummy_rx)
            })
            .expect("spawning fake HID worker");
        Self { rx, stop }
    }

    /// Drain pending events into `out`. Non-blocking.
    pub fn poll(&mut self, out: &mut Vec<GamepadEvent>) {
        while let Ok(ev) = self.rx.try_recv() {
            out.push(ev);
        }
    }
}

impl Drop for HidSource {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn worker_real(
    tx: Sender<GamepadEvent>,
    stop: Arc<AtomicBool>,
    params: CursorParams,
    disconnect_rx: crossbeam_channel::Receiver<()>,
) {
    let mut api = match hidapi::HidApi::new() {
        Ok(a) => a,
        Err(e) => {
            tracing::error!(error = %e, "hidapi init failed");
            return;
        }
    };
    while !stop.load(Ordering::SeqCst) {
        // device_list() returns a cached snapshot — must refresh
        // explicitly each iteration or a pad turned on after app
        // startup is invisible forever.
        if let Err(e) = api.refresh_devices() {
            tracing::warn!(error = %e, "hidapi refresh_devices failed");
        }
        let device = api
            .device_list()
            .find(|info| info.vendor_id() == DS_VID && info.product_id() == DS_PID)
            .and_then(|info| info.open_device(&api).ok());
        match device {
            Some(d) => {
                tracing::info!("DualSense opened, attempting 0x31 handshake");
                let _ = try_handshake(&d);
                let mut prev_buttons = [false; 25];
                let mut last_state: Option<DsState> = None;
                let mut touchpad = TouchpadState::default();
                let outcome = read_loop(
                    d, &tx, &mut last_state, &mut prev_buttons, &mut touchpad,
                    &params, &stop, &disconnect_rx,
                );
                // The HID handle (`d`) was moved into read_loop and is now
                // dropped, so Windows can cleanly drop the Bluetooth bond.
                // A user-requested disconnect means "turn the pad off": the
                // DualSense HID protocol has no power-off command, so power
                // it off at the Bluetooth-link layer (see `crate::platform`).
                if outcome == "manual-disconnect" {
                    match crate::platform::power_off_connected_dualsense() {
                        Ok(n) => tracing::info!(removed = n, "controller power-off requested"),
                        Err(e) => tracing::warn!(error = %e, "controller power-off failed"),
                    }
                }
                tracing::info!(?outcome, "read loop exited; back to Searching");
                let _ = tx.send(GamepadEvent::Disconnected);
            }
            None => {
                // While Searching with no pad available, still respect
                // disconnect signal (drain — it's a no-op but consumes the message)
                // and respect stop.
                let _ = disconnect_rx.try_recv();
                thread::sleep(ENUM_RETRY_INTERVAL);
            }
        }
    }
}

fn try_handshake(d: &hidapi::HidDevice) -> Result<()> {
    let mut buf = build_handshake_buffer();
    d.get_feature_report(&mut buf)
        .context("requesting calibration feature 0x05 to unlock 0x31 mode")?;
    Ok(())
}

fn read_loop(
    d: hidapi::HidDevice,
    tx: &Sender<GamepadEvent>,
    last_state: &mut Option<DsState>,
    prev_buttons: &mut [bool; 25],
    touchpad: &mut TouchpadState,
    params: &CursorParams,
    stop: &Arc<AtomicBool>,
    disconnect_rx: &crossbeam_channel::Receiver<()>,
) -> &'static str {
    let mut buf = [0u8; REPORT_LEN_BT];
    let mut consecutive_timeouts = 0u32;
    let mut emitted_connected = false;
    while !stop.load(Ordering::SeqCst) {
        // Manual disconnect check — drop device and return to Searching.
        if disconnect_rx.try_recv().is_ok() {
            return "manual-disconnect";
        }
        match d.read_timeout(&mut buf, READ_TIMEOUT_MS) {
            Ok(0) => {
                consecutive_timeouts += 1;
                if consecutive_timeouts >= DISCONNECT_AFTER_TIMEOUTS {
                    return "timeout";
                }
            }
            Ok(_n) => {
                consecutive_timeouts = 0;
                if let Some(s) = decode_31(&buf) {
                    if !emitted_connected {
                        let _ = tx.send(GamepadEvent::Connected);
                        emitted_connected = true;
                    }
                    diff_emit(last_state.as_ref(), &s, prev_buttons, tx);
                    process_touchpad(touchpad, &s, params, tx);
                    *last_state = Some(s);
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "HID read error");
                return "read-error";
            }
        }
    }
    "stop-signal"
}

fn worker_byte_stream(
    byte_rx: Receiver<Vec<u8>>,
    tx: Sender<GamepadEvent>,
    stop: Arc<AtomicBool>,
    params: CursorParams,
    disconnect_rx: crossbeam_channel::Receiver<()>,
) {
    use crossbeam_channel::select;
    let mut last_state: Option<DsState> = None;
    let mut prev_buttons = [false; 25];
    let mut touchpad = TouchpadState::default();
    let mut emitted_connected = false;
    let mut disconnect_alive = true;

    while !stop.load(Ordering::SeqCst) {
        if disconnect_alive {
            select! {
                recv(disconnect_rx) -> msg => {
                    if msg.is_err() {
                        // Sender dropped (e.g. test stub) — channel permanently
                        // closed; stop multiplexing it.
                        disconnect_alive = false;
                        continue;
                    }
                    let _ = tx.send(GamepadEvent::Disconnected);
                    emitted_connected = false;
                    last_state = None;
                    touchpad = TouchpadState::default();
                }
                recv(byte_rx) -> result => match result {
                    Ok(buf) => {
                        if let Some(s) = decode_31(&buf) {
                            if !emitted_connected {
                                let _ = tx.send(GamepadEvent::Connected);
                                emitted_connected = true;
                            }
                            diff_emit(last_state.as_ref(), &s, &mut prev_buttons, &tx);
                            process_touchpad(&mut touchpad, &s, &params, &tx);
                            last_state = Some(s);
                        }
                    }
                    Err(_) => {
                        let _ = tx.send(GamepadEvent::Disconnected);
                        return;
                    }
                }
            }
        } else {
            // disconnect_rx is closed — fall back to plain blocking recv on byte_rx.
            match byte_rx.recv() {
                Ok(buf) => {
                    if let Some(s) = decode_31(&buf) {
                        if !emitted_connected {
                            let _ = tx.send(GamepadEvent::Connected);
                            emitted_connected = true;
                        }
                        diff_emit(last_state.as_ref(), &s, &mut prev_buttons, &tx);
                        process_touchpad(&mut touchpad, &s, &params, &tx);
                        last_state = Some(s);
                    }
                }
                Err(_) => {
                    let _ = tx.send(GamepadEvent::Disconnected);
                    return;
                }
            }
        }
    }
}

fn diff_emit(
    prev: Option<&DsState>,
    cur: &DsState,
    prev_buttons: &mut [bool; 25],
    tx: &Sender<GamepadEvent>,
) {
    if prev.map_or(true, |p| p.stick_lx != cur.stick_lx) {
        let _ = tx.send(GamepadEvent::Stick { axis: 0, value: cur.stick_lx });
    }
    if prev.map_or(true, |p| p.stick_ly != cur.stick_ly) {
        let _ = tx.send(GamepadEvent::Stick { axis: 1, value: cur.stick_ly });
    }
    if prev.map_or(true, |p| p.stick_rx != cur.stick_rx) {
        let _ = tx.send(GamepadEvent::Stick { axis: 2, value: cur.stick_rx });
    }
    if prev.map_or(true, |p| p.stick_ry != cur.stick_ry) {
        let _ = tx.send(GamepadEvent::Stick { axis: 3, value: cur.stick_ry });
    }
    if prev.map_or(true, |p| p.trigger_l2 != cur.trigger_l2) {
        let _ = tx.send(GamepadEvent::Trigger { axis: 4, value: cur.trigger_l2 });
    }
    if prev.map_or(true, |p| p.trigger_r2 != cur.trigger_r2) {
        let _ = tx.send(GamepadEvent::Trigger { axis: 5, value: cur.trigger_r2 });
    }
    for (i, &down) in cur.buttons.iter().enumerate() {
        if down != prev_buttons[i] {
            let id = i as u32;
            let _ = tx.send(if down {
                GamepadEvent::ButtonDown(id)
            } else {
                GamepadEvent::ButtonUp(id)
            });
        }
    }
    prev_buttons.copy_from_slice(&cur.buttons);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quadrant_for_4_corners_axis_rect() {
        // Standard midpoint (960, 540) — 1920×1080 touchpad space.
        assert_eq!(quadrant_for(100,  100, 960, 540), QUAD_TL, "upper-left corner → TL (25)");
        assert_eq!(quadrant_for(1800, 100, 960, 540), QUAD_TR, "upper-right corner → TR (26)");
        assert_eq!(quadrant_for(100, 1000, 960, 540), QUAD_BL, "lower-left corner → BL (27)");
        assert_eq!(quadrant_for(1800, 1000, 960, 540), QUAD_BR, "lower-right corner → BR (28)");
    }

    #[test]
    fn quadrant_for_boundary_deterministic() {
        // Boundary at mid_x / mid_y. `x < mid_x` is false when x == mid_x → right column.
        // `y < mid_y` is false when y == mid_y → bottom row.
        assert_eq!(quadrant_for(960, 540, 960, 540), QUAD_BR, "exact midpoint → BR (right + bottom)");
        assert_eq!(quadrant_for(960, 100, 960, 540), QUAD_TR, "on X midline, upper → TR");
        assert_eq!(quadrant_for(100, 540, 960, 540), QUAD_BL, "on Y midline, left → BL");
    }

    #[test]
    fn hover_dedupe_same_quadrant_emits_once() {
        let mut state = TouchpadState::default();
        let params = CursorParams::default();
        let mut emitted: Vec<GamepadEvent> = Vec::new();
        for _ in 0..5 {
            process_touchpad_hover(true, 1500, 200, &mut state, &params, |ev| emitted.push(ev));
        }
        let hovers: Vec<_> = emitted.iter()
            .filter(|e| matches!(e, GamepadEvent::TouchpadHover { .. }))
            .collect();
        assert_eq!(hovers.len(), 1, "expected 1 emit for 5 same-quadrant frames");
    }

    #[test]
    fn hover_dedupe_quadrant_change_emits_again() {
        let mut state = TouchpadState::default();
        let params = CursorParams::default();
        let mut emitted: Vec<GamepadEvent> = Vec::new();
        process_touchpad_hover(true, 1500, 200, &mut state, &params, |ev| emitted.push(ev));  // TR
        process_touchpad_hover(true, 1500, 200, &mut state, &params, |ev| emitted.push(ev));  // same — skip
        process_touchpad_hover(true,  200, 200, &mut state, &params, |ev| emitted.push(ev));  // TL
        let hovers: Vec<_> = emitted.iter()
            .filter(|e| matches!(e, GamepadEvent::TouchpadHover { .. }))
            .collect();
        assert_eq!(hovers.len(), 2, "expected 2 emits: enter TR, then enter TL");
    }

    #[test]
    fn hover_lift_emits_sentinel() {
        let mut state = TouchpadState::default();
        let params = CursorParams::default();
        let mut emitted: Vec<GamepadEvent> = Vec::new();
        process_touchpad_hover(true, 1500, 200, &mut state, &params, |ev| emitted.push(ev));  // enter TR
        process_touchpad_hover(false,    0,   0, &mut state, &params, |ev| emitted.push(ev)); // lift
        let last_quadrant = emitted.iter().filter_map(|e| match e {
            GamepadEvent::TouchpadHover { quadrant, .. } => Some(*quadrant),
            _ => None,
        }).last().unwrap();
        assert_eq!(last_quadrant, HOVER_QUADRANT_NONE, "lift should emit sentinel quadrant");
    }

    #[test]
    fn hover_lift_then_reentry_emits_again() {
        let mut state = TouchpadState::default();
        let params = CursorParams::default();
        let mut emitted: Vec<GamepadEvent> = Vec::new();
        // Enter TR
        process_touchpad_hover(true, 1500, 200, &mut state, &params, |ev| emitted.push(ev));
        // Lift
        process_touchpad_hover(false,    0,   0, &mut state, &params, |ev| emitted.push(ev));
        // Re-enter same quadrant TR → must re-emit (proves dedupe state was reset)
        process_touchpad_hover(true, 1500, 200, &mut state, &params, |ev| emitted.push(ev));
        let hovers: Vec<_> = emitted.iter()
            .filter(|e| matches!(e, GamepadEvent::TouchpadHover { .. }))
            .collect();
        assert_eq!(hovers.len(), 3, "expected 3 emits: enter TR, sentinel, re-enter TR");
    }

    #[test]
    fn filter_cursor_delta_click_freeze_within_window_suppresses() {
        let mut state = TouchpadState::default();
        state.click_btn_held = true;
        state.click_btn_down_at = Some(Instant::now());
        state.click_anchor_pos = Some((500, 500));
        state.pos_buffer.push_back((501, 501, Instant::now()));  // 1px from anchor
        let params = CursorParams::default();
        let result = filter_cursor_delta(2, 2, &mut state, &params);
        assert_eq!(result, None, "within 80ms + < 15px → freeze");
    }

    #[test]
    fn filter_cursor_delta_click_freeze_exits_after_window() {
        let mut state = TouchpadState::default();
        state.click_btn_held = true;
        state.click_btn_down_at = Some(Instant::now() - Duration::from_millis(100));
        state.click_anchor_pos = Some((500, 500));
        state.pos_buffer.push_back((501, 501, Instant::now()));
        let params = CursorParams::default();
        params.set_deadzone_radius(0);  // bypass L2
        let result = filter_cursor_delta(5, 5, &mut state, &params);
        assert!(result.is_some(), "after 80ms window → unfreeze (drag enabled)");
    }

    #[test]
    fn filter_cursor_delta_click_freeze_exits_on_drag() {
        let mut state = TouchpadState::default();
        state.click_btn_held = true;
        state.click_btn_down_at = Some(Instant::now());
        state.click_anchor_pos = Some((500, 500));
        // Cumulative motion 20 px > CLICK_DRAG_EXIT_PX (15) → exit freeze
        state.pos_buffer.push_back((520, 500, Instant::now()));
        let params = CursorParams::default();
        params.set_deadzone_radius(0);
        let result = filter_cursor_delta(5, 0, &mut state, &params);
        assert!(result.is_some(), "cumulative > 15px → drag detected, unfreeze");
    }

    #[test]
    fn filter_cursor_delta_no_freeze_when_disabled() {
        let mut state = TouchpadState::default();
        state.click_btn_held = true;
        state.click_btn_down_at = Some(Instant::now());
        state.click_anchor_pos = Some((500, 500));
        state.pos_buffer.push_back((500, 500, Instant::now()));
        let params = CursorParams::default();
        params.set_click_freeze_enabled(false);
        params.set_deadzone_radius(0);
        let result = filter_cursor_delta(10, 10, &mut state, &params);
        assert!(result.is_some(), "click_freeze_enabled=false → pass through");
    }

    #[test]
    fn filter_deadzone_3_frames_below_radius_suppresses() {
        let mut state = TouchpadState::default();
        let params = CursorParams::default();  // deadzone_radius = 2
        // 3 consecutive frames at dx=1, dy=1 → mag² = 2 < 4 → all suppressed.
        assert_eq!(filter_cursor_delta(1, 1, &mut state, &params), None);
        assert_eq!(filter_cursor_delta(1, 1, &mut state, &params), None);
        assert_eq!(filter_cursor_delta(1, 1, &mut state, &params), None);
    }

    #[test]
    fn filter_deadzone_one_frame_above_radius_passes() {
        let mut state = TouchpadState::default();
        let params = CursorParams::default();
        assert_eq!(filter_cursor_delta(1, 1, &mut state, &params), None);
        let result = filter_cursor_delta(5, 5, &mut state, &params);
        assert!(result.is_some(), "5,5 above deadzone radius 2 should pass; got None");
    }

    #[test]
    fn filter_curve_slow_uses_slow_gain() {
        let mut state = TouchpadState::default();
        let params = CursorParams::default();
        params.set_deadzone_radius(0);  // disable L2 so 1 frame survives
        params.set_sensitivity(1.0);    // isolate gain
        let (dx, dy) = filter_cursor_delta(3, 0, &mut state, &params).unwrap();
        // mag = 3 < slow_threshold 5 → gain = 0.5 → 3 * 1.0 * 0.5 = 1.5 → 1 (truncated)
        assert_eq!(dx, 1, "slow region 3*0.5=1.5 → 1");
        assert_eq!(dy, 0);
    }

    #[test]
    fn filter_curve_fast_uses_fast_gain() {
        let mut state = TouchpadState::default();
        let params = CursorParams::default();
        params.set_deadzone_radius(0);
        params.set_sensitivity(1.0);
        let (dx, dy) = filter_cursor_delta(30, 0, &mut state, &params).unwrap();
        // mag = 30 > fast_threshold 20 → gain = 1.5 → 30 * 1.5 = 45
        assert_eq!(dx, 45);
        assert_eq!(dy, 0);
    }

    #[test]
    fn filter_curve_mid_uses_linear_interp() {
        let mut state = TouchpadState::default();
        let params = CursorParams::default();
        params.set_deadzone_radius(0);
        params.set_sensitivity(1.0);
        // dx=10 dy=0 → mag = 10, t = (10-5)/15 = 1/3 → gain = 0.5 + (1/3)*1 = 0.833
        // dx_out = 10 * 0.833 = 8.33 → 8
        let (dx, dy) = filter_cursor_delta(10, 0, &mut state, &params).unwrap();
        assert_eq!(dx, 8, "mid region 10 * 0.833 = 8.33 → 8");
        assert_eq!(dy, 0);
    }

    #[test]
    fn worker_byte_stream_honors_disconnect_signal() {
        use std::sync::atomic::AtomicBool;
        use std::time::Duration;
        let (_byte_tx, byte_rx) = crossbeam_channel::unbounded::<Vec<u8>>();
        let (evt_tx, evt_rx) = crossbeam_channel::unbounded::<GamepadEvent>();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = stop.clone();
        let (disc_tx, disc_rx) = crossbeam_channel::unbounded::<()>();
        let params = CursorParams::default();
        let h = std::thread::spawn(move || {
            super::worker_byte_stream(byte_rx, evt_tx, stop_clone, params, disc_rx);
        });
        // Send disconnect immediately
        disc_tx.send(()).expect("send disconnect");
        // Worker should emit Disconnected within 500ms
        let ev = evt_rx.recv_timeout(Duration::from_millis(500))
            .expect("worker did not emit Disconnected within 500ms");
        assert!(matches!(ev, GamepadEvent::Disconnected),
            "expected Disconnected, got {:?}", ev);
        // Worker still alive — verify by stopping it now and joining cleanly
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        // Send something to unblock the select! so it observes `stop`
        disc_tx.send(()).ok();
        h.join().expect("worker thread should finish cleanly");
    }

    #[test]
    fn stable_pos_picks_closest_to_lookback_target() {
        let mut state = TouchpadState::default();
        let t0 = Instant::now();
        // Push positions at t0, t0+10ms, t0+25ms, t0+40ms, t0+60ms
        state.pos_buffer.push_back((100, 100, t0));
        state.pos_buffer.push_back((110, 100, t0 + Duration::from_millis(10)));
        state.pos_buffer.push_back((130, 100, t0 + Duration::from_millis(25)));
        state.pos_buffer.push_back((160, 100, t0 + Duration::from_millis(40)));
        state.pos_buffer.push_back((200, 100, t0 + Duration::from_millis(60)));
        // Lookback 30ms from t0+60ms → target t0+30ms → closest entry t0+25ms = (130, 100)
        let now = t0 + Duration::from_millis(60);
        let stable = stable_pos_for_click(&state, now, Duration::from_millis(30));
        assert_eq!(stable, Some((130, 100)));
    }
}
