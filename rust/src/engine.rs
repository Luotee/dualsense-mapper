//! Engine: long-running mapper loop with a thread-safe handle for the GUI.
//!
//! v0.1.x ran the loop directly from `app::run`. The GUI needs to mutate
//! config while the loop runs, pause it, suppress synth during key
//! capture, and shut it down. This module wraps that into:
//!
//!   * `Engine`      — owns the thread.
//!   * `Handle`      — what the GUI holds; clones cheaply (`Arc` inside).
//!   * `EngineEvent` — what the loop emits for the activity log.

use crate::config::Config;
use crate::gamepad::{ControllerKind, CursorParams, GamepadSource};
use crate::keyboard::KeyboardSink;
use crate::macro_engine::MacroEngine;
use crate::mapper::{KeyAction, Mapper};
use crate::mouse::MouseSink;
use crate::safety::{self, SharedKeyState};
use anyhow::Result;
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::thread::{self, JoinHandle};
use std::time::Duration;

// ─── Public event type ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum EngineEvent {
    ControllerConnected { name: String, transport: String, kind: ControllerKind },
    ControllerDisconnected,
    ButtonDown { id: u32 },
    ButtonUp   { id: u32 },
    KeyEmit    { ts_ms: u64, key: String, action: &'static str /* "down"|"up" */ },
    MacroStart { ts_ms: u64, name: String },
    MacroEnd   { ts_ms: u64, name: String, completed: bool },
    /// Diagnostic — emitted once per touchpad click rising edge with
    /// the raw finger position captured at that instant.
    TouchpadClick { raw_x: u16, raw_y: u16, quadrant: u32 },
    /// Diagnostic — emitted per HID frame on touchpad hover quadrant change.
    /// `quadrant = 255` is the sentinel for "finger lifted, clear UI hover".
    TouchpadHover { raw_x: u16, raw_y: u16, quadrant: u32 },
}

/// Snapshot of the controller's current connection state. Polled by the GUI
/// on first load (since Tauri events don't replay missed emissions) and
/// mutated by the run loop on every Connected/Disconnected.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ControllerStatus {
    pub name: String,
    pub transport: String,
    pub kind: ControllerKind,
    pub warnings: Vec<String>,
}

// ─── ConfigWriteGuard — bumps generation on drop ─────────────────────────────

/// An RAII wrapper around `RwLockWriteGuard<Config>` that increments
/// `config_generation` when dropped, so the run loop knows to rebuild
/// the `Mapper` on the next tick.
pub struct ConfigWriteGuard<'a> {
    guard: RwLockWriteGuard<'a, Config>,
    generation: &'a AtomicU64,
}

impl<'a> std::ops::Deref for ConfigWriteGuard<'a> {
    type Target = Config;
    fn deref(&self) -> &Config { &self.guard }
}

impl<'a> std::ops::DerefMut for ConfigWriteGuard<'a> {
    fn deref_mut(&mut self) -> &mut Config { &mut self.guard }
}

impl<'a> Drop for ConfigWriteGuard<'a> {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }
}

// ─── Handle internals ────────────────────────────────────────────────────────

struct HandleInner {
    config: RwLock<Config>,
    /// Incremented by ConfigWriteGuard::drop; the run loop tracks the last
    /// seen value and rebuilds the Mapper when they differ.
    config_generation: AtomicU64,
    paused: AtomicBool,
    capture_active: AtomicBool,
    shutdown: AtomicBool,
    event_tx: Sender<EngineEvent>,
    event_rx: Receiver<EngineEvent>,
    key_state: SharedKeyState,
    /// Current controller connection state. Set/cleared by the run loop on
    /// every Connected/Disconnected event. Exposed via `Handle::current_status`
    /// so the frontend can synchronously query "is a controller already
    /// connected?" on init without waiting to miss a startup-race event.
    current_status: RwLock<Option<ControllerStatus>>,
    /// Test-only: sender for the fake gamepad source. `None` in production.
    #[doc(hidden)]
    fake_tx: Mutex<Option<crossbeam_channel::Sender<crate::gamepad::GamepadEvent>>>,
    /// Live touchpad cursor parameters, shared with the HID worker.
    /// `set_settings` mutates these through the handle so changes take
    /// effect on the next decoded frame without rebuilding the engine.
    cursor_params: CursorParams,
    /// Manual disconnect signal — Tauri `disconnect_gamepad` IPC sends
    /// through this. HID worker honors it to release the hidapi device
    /// handle and return to Searching state without exiting the thread.
    disconnect_signal: crossbeam_channel::Sender<()>,
}

// ─── Handle (GUI-side) ───────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Handle {
    inner: Arc<HandleInner>,
}

impl Handle {
    /// Live touchpad cursor parameters. The HID worker reads through
    /// this every frame; `set_settings` writes through it on the
    /// engine thread so changes take effect without a config rebuild.
    pub fn cursor_params(&self) -> CursorParams {
        self.inner.cursor_params.clone()
    }

    pub fn set_paused(&self, v: bool) {
        self.inner.paused.store(v, Ordering::SeqCst);
    }
    pub fn is_paused(&self) -> bool {
        self.inner.paused.load(Ordering::SeqCst)
    }
    pub fn set_capture_active(&self, v: bool) {
        self.inner.capture_active.store(v, Ordering::SeqCst);
    }
    pub fn is_capture_active(&self) -> bool {
        self.inner.capture_active.load(Ordering::SeqCst)
    }

    pub fn config_read(&self) -> RwLockReadGuard<'_, Config> {
        self.inner.config.read().unwrap()
    }

    pub fn config_write(&self) -> ConfigWriteGuard<'_> {
        ConfigWriteGuard {
            guard: self.inner.config.write().unwrap(),
            generation: &self.inner.config_generation,
        }
    }

    /// Current controller connection state — `Some` if a gamepad is plugged in
    /// (or was at startup), `None` otherwise. Frontend uses this on first load
    /// to avoid the Tauri-event startup race (events don't replay).
    pub fn current_status(&self) -> Option<ControllerStatus> {
        self.inner.current_status.read().unwrap().clone()
    }

    pub fn drain_events(&self) -> Vec<EngineEvent> {
        let mut v = Vec::new();
        while let Ok(e) = self.inner.event_rx.try_recv() {
            v.push(e);
        }
        v
    }

    pub fn held_keys_count(&self) -> usize {
        self.inner.key_state.lock().unwrap_or_else(|p| p.into_inner()).len_held()
    }

    /// Return a clone of the engine's key-state Arc. Used by `app::run` to
    /// register the global panic-hook state after the engine is spawned.
    pub fn key_state(&self) -> crate::safety::SharedKeyState {
        self.inner.key_state.clone()
    }

    /// Inject a fake ButtonDown event. Works only when spawned via
    /// `Engine::spawn_with_fake_gamepad`; no-op otherwise.
    #[doc(hidden)]
    pub fn fake_button_down(&self, id: u32) {
        if let Ok(guard) = self.inner.fake_tx.lock() {
            if let Some(ref tx) = *guard {
                let _ = tx.send(crate::gamepad::GamepadEvent::ButtonDown(id));
            }
        }
    }

    /// Inject an arbitrary fake gamepad event. Works only when spawned via
    /// `Engine::spawn_with_fake_gamepad`; no-op otherwise.
    #[doc(hidden)]
    pub fn fake_event(&self, ev: crate::gamepad::GamepadEvent) {
        if let Ok(guard) = self.inner.fake_tx.lock() {
            if let Some(ref tx) = *guard {
                let _ = tx.send(ev);
            }
        }
    }

    /// Inject a fake ButtonUp event. Works only when spawned via
    /// `Engine::spawn_with_fake_gamepad`; no-op otherwise.
    #[doc(hidden)]
    pub fn fake_button_up(&self, id: u32) {
        if let Ok(guard) = self.inner.fake_tx.lock() {
            if let Some(ref tx) = *guard {
                let _ = tx.send(crate::gamepad::GamepadEvent::ButtonUp(id));
            }
        }
    }

    /// Manual disconnect — send a signal to the HID worker to drop its
    /// current device handle and return to Searching. The thread does
    /// NOT exit; a new controller (the same or different) reconnects
    /// via the normal handshake path on next button press.
    pub fn disconnect_current_device(&self) {
        let _ = self.inner.disconnect_signal.send(());
    }
}

// ─── Engine (owns the thread) ────────────────────────────────────────────────

pub struct Engine {
    thread: Option<JoinHandle<Result<()>>>,
    handle: Handle,
}

impl Engine {
    /// Spawn with a real gamepad source.
    pub fn spawn(cfg: Config, dry_run: bool) -> Result<Self> {
        let cursor_params = CursorParams::with_midpoints(
            cfg.touchpad_cursor_sensitivity,
            cfg.touchpad_cursor_enabled,
            cfg.touchpad_midpoint_x,
            cfg.touchpad_midpoint_y,
        );
        let (disconnect_tx, disconnect_rx) = crossbeam_channel::bounded::<()>(4);
        let src = GamepadSource::new(cursor_params.clone(), disconnect_rx)?;
        Self::spawn_inner(cfg, dry_run, src, cursor_params, disconnect_tx)
    }

    /// Spawn with a fake gamepad source. Events are injected via
    /// `Handle::fake_button_down` / `Handle::fake_button_up`.
    /// Not part of the stable public API — used by integration tests.
    #[doc(hidden)]
    pub fn spawn_with_fake_gamepad(cfg: Config) -> Result<Self> {
        let cursor_params = CursorParams::with_midpoints(
            cfg.touchpad_cursor_sensitivity,
            cfg.touchpad_cursor_enabled,
            cfg.touchpad_midpoint_x,
            cfg.touchpad_midpoint_y,
        );
        let (fake_tx, fake_rx) = unbounded::<crate::gamepad::GamepadEvent>();
        let src = GamepadSource::fake(fake_rx);
        let (disconnect_tx, _disconnect_rx) = crossbeam_channel::bounded::<()>(4);
        let engine = Self::spawn_inner(cfg, /*dry_run=*/true, src, cursor_params, disconnect_tx)?;
        // Install the sender into HandleInner.
        *engine.handle.inner.fake_tx.lock().unwrap() = Some(fake_tx);
        Ok(engine)
    }

    fn spawn_inner(
        cfg: Config,
        dry_run: bool,
        src: GamepadSource,
        cursor_params: CursorParams,
        disconnect_tx: crossbeam_channel::Sender<()>,
    ) -> Result<Self> {
        let key_state = safety::shared();
        let (event_tx, event_rx) = unbounded::<EngineEvent>();

        let inner = Arc::new(HandleInner {
            config: RwLock::new(cfg),
            config_generation: AtomicU64::new(0),
            paused: AtomicBool::new(false),
            capture_active: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
            event_tx,
            event_rx,
            key_state: key_state.clone(),
            fake_tx: Mutex::new(None),
            current_status: RwLock::new(None),
            cursor_params,
            disconnect_signal: disconnect_tx,
        });

        let handle = Handle { inner: inner.clone() };

        let thread = thread::spawn(move || run_loop(inner, src, dry_run));
        Ok(Engine { thread: Some(thread), handle })
    }

    pub fn handle(&self) -> Handle {
        self.handle.clone()
    }

    /// Signal shutdown and block until the engine thread exits.
    pub fn shutdown(mut self) {
        self.handle.inner.shutdown.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

// ─── Run loop (engine-thread) ─────────────────────────────────────────────────

fn run_loop(h: Arc<HandleInner>, mut source: GamepadSource, dry_run: bool) -> Result<()> {
    // Read initial config values before the loop.
    let (tick_jitter_ms, min_press_ms) = {
        let cfg = h.config.read().unwrap();
        (cfg.tick_jitter_ms, cfg.min_press_ms)
    };

    let mut sink = KeyboardSink::new(h.key_state.clone(), tick_jitter_ms, min_press_ms, dry_run)
        .expect("KeyboardSink::new failed");
    let mut mouse_sink = MouseSink::new(h.key_state.clone(), dry_run)
        .expect("MouseSink::new failed");

    // Set up macro → keyboard channel (std::sync::mpsc, matching MacroEngine).
    let (macro_tx, macro_rx) = std::sync::mpsc::channel::<KeyAction>();
    let mut macros = MacroEngine::new(macro_tx);

    // Build the initial mapper.
    let mut mapper = {
        let cfg = h.config.read().unwrap();
        Mapper::new(cfg.clone())
    };
    let mut last_config_gen = h.config_generation.load(Ordering::SeqCst);

    let mut last_paused = false;
    let mut gp_events: Vec<crate::gamepad::GamepadEvent> = Vec::with_capacity(32);

    // Per-controller mapping gate. `None` (no controller seen yet, or after
    // Disconnected) means mapping ON.
    let mut current_kind: Option<ControllerKind> = None;
    let mut mapping_on = true;

    while !h.shutdown.load(Ordering::SeqCst) {
        // ── Pause / unpause edge ─────────────────────────────────────────────
        let paused = h.paused.load(Ordering::SeqCst);
        if paused && !last_paused {
            // Drain all macros and held keys when pausing.
            drain_and_release(&mut macros, &macro_rx, &mut sink, &mut mouse_sink, mapper.config(), &h.event_tx);
        }
        last_paused = paused;

        // ── Config change: rebuild mapper ────────────────────────────────────
        let current_gen = h.config_generation.load(Ordering::SeqCst);
        if current_gen != last_config_gen {
            let cfg = h.config.read().unwrap();
            mapper = Mapper::new(cfg.clone());
            last_config_gen = current_gen;
        }

        // ── Mapping ON→OFF edge: release everything ──────────────────────────
        let now_on = mapping_enabled(&h, current_kind);
        if mapping_on && !now_on {
            drain_and_release(&mut macros, &macro_rx, &mut sink, &mut mouse_sink, mapper.config(), &h.event_tx);
        }
        mapping_on = now_on;

        if !paused {
            // ── Poll gamepad ─────────────────────────────────────────────────
            gp_events.clear();
            source.poll(&mut gp_events);

            for ev in gp_events.drain(..) {
                // Always emit raw gamepad events (even when capture_active).
                match &ev {
                    crate::gamepad::GamepadEvent::ButtonDown(id) => {
                        let _ = h.event_tx.send(EngineEvent::ButtonDown { id: *id });
                    }
                    crate::gamepad::GamepadEvent::ButtonUp(id) => {
                        let _ = h.event_tx.send(EngineEvent::ButtonUp { id: *id });
                    }
                    crate::gamepad::GamepadEvent::Connected(kind) => {
                        let kind = *kind;
                        let status = ControllerStatus {
                            name: kind.display_name().to_string(),
                            transport: "BT".to_string(),
                            kind,
                            warnings: vec![],
                        };
                        *h.current_status.write().unwrap() = Some(status.clone());
                        let _ = h.event_tx.send(EngineEvent::ControllerConnected {
                            name: status.name,
                            transport: status.transport,
                            kind,
                        });
                        current_kind = Some(kind);
                        let now_on = mapping_enabled(&h, current_kind);
                        if mapping_on && !now_on {
                            drain_and_release(&mut macros, &macro_rx, &mut sink, &mut mouse_sink, mapper.config(), &h.event_tx);
                        }
                        mapping_on = now_on;
                    }
                    crate::gamepad::GamepadEvent::Disconnected => {
                        current_kind = None;
                        *h.current_status.write().unwrap() = None;
                        let _ = h.event_tx.send(EngineEvent::ControllerDisconnected);
                    }
                    _ => {}
                }

                // Gate synth on capture_active.
                if h.capture_active.load(Ordering::SeqCst) {
                    continue;
                }

                let cfg = h.config.read().unwrap();
                let actions = mapper.handle(ev);
                drop(cfg); // release lock before execute calls into sink
                for (id, pressed) in mapper.take_visual_transitions() {
                    let _ = if pressed {
                        h.event_tx.send(EngineEvent::ButtonDown { id })
                    } else {
                        h.event_tx.send(EngineEvent::ButtonUp { id })
                    };
                }
                for action in &actions {
                    // Mapping OFF: keep the touchpad diagnostics, synthesize nothing.
                    if !mapping_on && !is_diagnostic(action) {
                        continue;
                    }
                    execute_action(action, &mut sink, &mut mouse_sink, &mut macros, mapper.config(), &h.event_tx);
                }
            }

            // ── Drain macro-emitted key actions ──────────────────────────────
            while let Ok(action) = macro_rx.try_recv() {
                // Mapping OFF: a macro cancelled on the ON→OFF edge may still
                // race a late Press/Release in; drop it (sink was already reset).
                if !mapping_on && !is_diagnostic(&action) {
                    continue;
                }
                execute_action(&action, &mut sink, &mut mouse_sink, &mut macros, mapper.config(), &h.event_tx);
            }
        }

        thread::sleep(Duration::from_millis(8));
    }

    // ── Graceful shutdown ────────────────────────────────────────────────────
    macros.stop_all();
    // Drain any final macro releases.
    while let Ok(action) = macro_rx.try_recv() {
        execute_action(&action, &mut sink, &mut mouse_sink, &mut macros, mapper.config(), &h.event_tx);
    }
    // sink.drop() will call release_all_held() — that is the iron-rule guarantee.
    // We explicitly drop to make the intent clear.
    drop(sink);
    drop(mouse_sink);
    Ok(())
}

/// Whether mapping is ON for the connected controller. Reads the live config
/// (not the mapper's snapshot) so an IPC toggle takes effect on the next tick.
/// No known controller → ON.
fn mapping_enabled(h: &HandleInner, kind: Option<ControllerKind>) -> bool {
    kind.map_or(true, |k| h.config.read().unwrap().mapping_enabled_for(k))
}

/// Actions that synthesize no input — the only ones executed while mapping
/// is OFF.
fn is_diagnostic(action: &KeyAction) -> bool {
    matches!(action, KeyAction::TouchpadClick { .. } | KeyAction::TouchpadHover { .. })
}

/// Stop every macro, run the releases they drain, then release every held
/// key and mouse button (iron rules 2–4). Shared by the pause edge and the
/// mapping ON→OFF edge.
fn drain_and_release(
    macros: &mut MacroEngine,
    macro_rx: &std::sync::mpsc::Receiver<KeyAction>,
    sink: &mut KeyboardSink,
    mouse_sink: &mut MouseSink,
    cfg: &Config,
    event_tx: &Sender<EngineEvent>,
) {
    macros.stop_all();
    // Drain macro-emitted releases so sink's refcounts are balanced.
    while let Ok(action) = macro_rx.try_recv() {
        execute_action(&action, sink, mouse_sink, macros, cfg, event_tx);
    }
    sink.release_all_held();
    mouse_sink.release_all_held();
}

fn execute_action(
    action: &KeyAction,
    sink: &mut KeyboardSink,
    mouse_sink: &mut MouseSink,
    macros: &mut MacroEngine,
    cfg: &Config,
    event_tx: &Sender<EngineEvent>,
) {
    let now_ms = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    };
    match action {
        KeyAction::Press(k) => {
            let _ = sink.press(k);
            let _ = event_tx.send(EngineEvent::KeyEmit {
                ts_ms: now_ms(),
                key: k.clone(),
                action: "down",
            });
        }
        KeyAction::Release(k) => {
            let _ = sink.release(k);
            let _ = event_tx.send(EngineEvent::KeyEmit {
                ts_ms: now_ms(),
                key: k.clone(),
                action: "up",
            });
        }
        KeyAction::MacroStart { name, source_id } => {
            if let Some(def) = cfg.macros.get(name) {
                macros.start(*source_id, def.clone());
                let _ = event_tx.send(EngineEvent::MacroStart {
                    ts_ms: now_ms(),
                    name: name.clone(),
                });
            } else {
                tracing::error!(name = %name, "macro not found in config");
            }
        }
        KeyAction::MacroStop { source_id } => {
            macros.stop(*source_id);
            // MacroEnd with completed=false (interrupted by button release).
            // We don't track name per source_id here; emit a best-effort event.
        }
        KeyAction::MousePress(b) => {
            let _ = mouse_sink.click_press(*b);
        }
        KeyAction::MouseRelease(b) => {
            let _ = mouse_sink.click_release(*b);
        }
        KeyAction::MouseMove { dx, dy } => {
            let _ = mouse_sink.move_rel(*dx, *dy);
        }
        KeyAction::TouchpadClick { raw_x, raw_y, quadrant } => {
            let _ = event_tx.send(EngineEvent::TouchpadClick {
                raw_x: *raw_x,
                raw_y: *raw_y,
                quadrant: *quadrant,
            });
        }
        KeyAction::TouchpadHover { raw_x, raw_y, quadrant } => {
            let _ = event_tx.send(EngineEvent::TouchpadHover {
                raw_x: *raw_x,
                raw_y: *raw_y,
                quadrant: *quadrant,
            });
        }
    }
}
