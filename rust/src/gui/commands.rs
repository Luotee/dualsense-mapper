//! Tauri IPC commands exposed to the frontend.
//!
//! Every function in this module is registered via `generate_handler![]` in
//! `runtime.rs`. JS invokes them with `window.__TAURI__.core.invoke(name, args)`.
//!
//! Iron rule #9: commands are the gate between JS and Rust. JS may NOT
//! synthesise keys, may NOT decide mappings. Read-only commands (like
//! `get_config`) are trivially safe. Write commands go through `safety` +
//! engine.
//!
//! Pattern: each command has a `*_impl` sibling that is Tauri-free and can be
//! called directly from integration tests (which cannot construct
//! `tauri::State`). The `#[tauri::command]` wrappers are gated on the `gui`
//! feature; the `*_impl` functions are always compiled.

use crate::config::{Binding, ButtonEntry, Config, MacroDef};
use crate::config_io::{write_atomic, ConfigDoc};
use crate::engine::Handle;
use serde::{Deserialize, Serialize};
use std::path::Path;
#[cfg(feature = "gui")]
use std::path::PathBuf;

// ─── Tauri command wrappers (gui feature only) ───────────────────────────────

#[cfg(feature = "gui")]
use tauri::State;
#[cfg(feature = "gui")]
use tauri::Emitter as _;

/// Return the current live config.
///
/// The config is held in a `RwLock` inside the engine's `Handle`. We clone
/// it here so the lock is not held across the async serialisation boundary.
#[cfg_attr(feature = "gui", tauri::command)]
#[cfg(feature = "gui")]
pub fn get_config(engine: State<'_, Handle>) -> Result<Config, String> {
    Ok(engine.config_read().clone())
}

/// Tauri command: replace one button binding, validate, atomically write to
/// disk, and hot-rebind the live engine — all or nothing.
///
/// On validation failure the disk is untouched and the live config unchanged.
#[cfg_attr(feature = "gui", tauri::command)]
#[cfg(feature = "gui")]
pub fn set_binding(
    app: tauri::AppHandle,
    engine: State<'_, Handle>,
    config_path: State<'_, PathBuf>,
    id: u32,
    entry: ButtonEntry,
) -> Result<(), String> {
    set_binding_impl(&*engine, &config_path, id, entry).map_err(|e| format!("{e:#}"))?;
    let _ = app.emit("config-changed", serde_json::json!({"source": "ipc"}));
    Ok(())
}

// ─── Pure *_impl helpers (always compiled, no Tauri dependency) ──────────────

/// Pure implementation of `get_config` callable without `tauri::State`.
///
/// Exposed for integration tests; the Tauri wrapper above delegates here.
#[allow(dead_code)]
pub fn get_config_impl(engine: &Handle) -> Config {
    engine.config_read().clone()
}

/// Pure implementation of `set_binding` callable without `tauri::State`.
///
/// Loads the on-disk config via `ConfigDoc`, patches the button entry,
/// validates the result, and only then atomically writes to disk and
/// hot-rebinds the live engine. Any failure leaves both disk and engine
/// state unchanged.
pub fn set_binding_impl(
    engine: &Handle,
    config_path: &Path,
    id: u32,
    entry: ButtonEntry,
) -> anyhow::Result<()> {
    let mut doc = ConfigDoc::load(config_path)?;
    doc.replace_button(id, entry.clone());
    doc.validate()?;
    write_atomic(config_path, &doc)?;
    engine.config_write().buttons.insert(id.to_string(), entry);
    Ok(())
}

// ─── set_macro ───────────────────────────────────────────────────────────────

/// Tauri command: insert or replace a named macro, validate, atomically write
/// to disk, and hot-rebind the live engine.
#[cfg_attr(feature = "gui", tauri::command)]
#[cfg(feature = "gui")]
pub fn set_macro(
    app: tauri::AppHandle,
    engine: State<'_, Handle>,
    config_path: State<'_, PathBuf>,
    name: String,
    def: MacroDef,
) -> Result<(), String> {
    set_macro_impl(&*engine, &config_path, name, def).map_err(|e| format!("{e:#}"))?;
    let _ = app.emit("config-changed", serde_json::json!({"source": "ipc"}));
    Ok(())
}

/// Pure implementation of `set_macro` callable without `tauri::State`.
pub fn set_macro_impl(
    engine: &Handle,
    config_path: &Path,
    name: String,
    def: MacroDef,
) -> anyhow::Result<()> {
    let mut doc = ConfigDoc::load(config_path)?;
    let mut macros = doc.typed().macros.clone();
    macros.insert(name, def);
    doc.replace_macros(macros);
    doc.validate()?;
    write_atomic(config_path, &doc)?;
    engine.config_write().macros = doc.typed().macros.clone();
    Ok(())
}

// ─── delete_macro ────────────────────────────────────────────────────────────

/// Tauri command: delete a named macro, checking it is not referenced by any
/// button binding before removal.
#[cfg_attr(feature = "gui", tauri::command)]
#[cfg(feature = "gui")]
pub fn delete_macro(
    app: tauri::AppHandle,
    engine: State<'_, Handle>,
    config_path: State<'_, PathBuf>,
    name: String,
) -> Result<(), String> {
    delete_macro_impl(&*engine, &config_path, &name).map_err(|e| format!("{e:#}"))?;
    let _ = app.emit("config-changed", serde_json::json!({"source": "ipc"}));
    Ok(())
}

/// Pure implementation of `delete_macro` callable without `tauri::State`.
///
/// Returns an error if any button binding references the macro (referential
/// integrity check). The error message names both the macro and the button id.
pub fn delete_macro_impl(
    engine: &Handle,
    config_path: &Path,
    name: &str,
) -> anyhow::Result<()> {
    let mut doc = ConfigDoc::load(config_path)?;
    // Referential check: refuse if any button binding points at this macro.
    let bound_to: Vec<String> = doc
        .typed()
        .buttons
        .iter()
        .filter_map(|(id, entry)| match &entry.binding {
            Binding::Macro(n) if n == name => Some(id.clone()),
            _ => None,
        })
        .collect();
    if !bound_to.is_empty() {
        anyhow::bail!(
            "cannot delete macro '{name}' — bound by button {}",
            bound_to.join(", ")
        );
    }
    let mut macros = doc.typed().macros.clone();
    macros.remove(name);
    doc.replace_macros(macros);
    doc.validate()?;
    write_atomic(config_path, &doc)?;
    engine.config_write().macros = doc.typed().macros.clone();
    Ok(())
}

// ─── rename_macro ────────────────────────────────────────────────────────────

/// Tauri command: rename a macro and update all button bindings that reference
/// the old name.
#[cfg_attr(feature = "gui", tauri::command)]
#[cfg(feature = "gui")]
pub fn rename_macro(
    app: tauri::AppHandle,
    engine: State<'_, Handle>,
    config_path: State<'_, PathBuf>,
    old: String,
    new: String,
) -> Result<(), String> {
    rename_macro_impl(&*engine, &config_path, &old, &new).map_err(|e| format!("{e:#}"))?;
    let _ = app.emit("config-changed", serde_json::json!({"source": "ipc"}));
    Ok(())
}

/// Pure implementation of `rename_macro` callable without `tauri::State`.
///
/// Renames the macro in the macros map and rewrites every button binding that
/// referenced the old name so referential integrity is preserved.
pub fn rename_macro_impl(
    engine: &Handle,
    config_path: &Path,
    old: &str,
    new: &str,
) -> anyhow::Result<()> {
    if old == new {
        return Ok(());
    }
    let mut doc = ConfigDoc::load(config_path)?;
    if !doc.typed().macros.contains_key(old) {
        anyhow::bail!("macro '{old}' does not exist");
    }
    if doc.typed().macros.contains_key(new) {
        anyhow::bail!("macro '{new}' already exists");
    }
    // Rename in macros map.
    let mut macros = doc.typed().macros.clone();
    let def = macros.remove(old).expect("just checked");
    macros.insert(new.to_string(), def);
    doc.replace_macros(macros);
    // Update every button binding that points at the old name.
    let buttons_snapshot = doc.typed().buttons.clone();
    for (id_str, entry) in &buttons_snapshot {
        if let Binding::Macro(ref n) = entry.binding {
            if n == old {
                let id: u32 = id_str.parse().expect("button keys are u32 strings");
                let updated = ButtonEntry {
                    label: entry.label.clone(),
                    binding: Binding::Macro(new.to_string()),
                };
                doc.replace_button(id, updated);
            }
        }
    }
    doc.validate()?;
    write_atomic(config_path, &doc)?;
    *engine.config_write() = doc.typed().clone();
    Ok(())
}

// ─── Settings ────────────────────────────────────────────────────────────────

/// Shape of the Settings tab form on the frontend. Deserialised from a
/// `#[tauri::command]` call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub deadzone: f32,
    pub trigger_threshold: f32,
    pub min_press_ms: [u32; 2],
    pub tick_jitter_ms: [u32; 2],
    pub log_events: bool,
    #[serde(default = "default_touchpad_cursor_enabled")]
    pub touchpad_cursor_enabled: bool,
    #[serde(default = "default_touchpad_cursor_sensitivity")]
    pub touchpad_cursor_sensitivity: f32,
    #[serde(default = "default_touchpad_midpoint_x")]
    pub touchpad_midpoint_x: u16,
    #[serde(default = "default_touchpad_midpoint_y")]
    pub touchpad_midpoint_y: u16,
    #[serde(default = "default_touchpad_accel_slow_threshold")]
    pub touchpad_accel_slow_threshold: u32,
    #[serde(default = "default_touchpad_accel_fast_threshold")]
    pub touchpad_accel_fast_threshold: u32,
    #[serde(default = "default_touchpad_accel_gain_slow")]
    pub touchpad_accel_gain_slow: f32,
    #[serde(default = "default_touchpad_accel_gain_fast")]
    pub touchpad_accel_gain_fast: f32,
    #[serde(default = "default_touchpad_deadzone_radius")]
    pub touchpad_deadzone_radius: u32,
    #[serde(default = "default_touchpad_click_freeze_enabled")]
    pub touchpad_click_freeze_enabled: bool,
}

fn default_touchpad_cursor_enabled() -> bool { true }
fn default_touchpad_cursor_sensitivity() -> f32 { 1.5 }
fn default_touchpad_midpoint_x() -> u16 { 960 }
fn default_touchpad_midpoint_y() -> u16 { 540 }
fn default_touchpad_accel_slow_threshold() -> u32 { 5 }
fn default_touchpad_accel_fast_threshold() -> u32 { 20 }
fn default_touchpad_accel_gain_slow() -> f32 { 0.5 }
fn default_touchpad_accel_gain_fast() -> f32 { 1.5 }
fn default_touchpad_deadzone_radius() -> u32 { 2 }
fn default_touchpad_click_freeze_enabled() -> bool { true }

impl Settings {
    /// Spec §9 — the factory defaults that ship with the example config.
    pub fn defaults() -> Self {
        Self {
            deadzone: 0.4,
            trigger_threshold: 0.5,
            min_press_ms: [8, 25],
            tick_jitter_ms: [0, 3],
            log_events: true,
            touchpad_cursor_enabled: true,
            touchpad_cursor_sensitivity: 1.5,
            touchpad_midpoint_x: 960,
            touchpad_midpoint_y: 540,
            touchpad_accel_slow_threshold: 5,
            touchpad_accel_fast_threshold: 20,
            touchpad_accel_gain_slow: 0.5,
            touchpad_accel_gain_fast: 1.5,
            touchpad_deadzone_radius: 2,
            touchpad_click_freeze_enabled: true,
        }
    }
}

/// Tauri command: apply settings from the frontend Settings tab, validate,
/// atomically write to disk, and hot-reload the live engine.
#[cfg_attr(feature = "gui", tauri::command)]
#[cfg(feature = "gui")]
pub fn set_settings(
    app: tauri::AppHandle,
    engine: State<'_, Handle>,
    config_path: State<'_, PathBuf>,
    settings: Settings,
) -> Result<(), String> {
    set_settings_impl(&*engine, &config_path, settings).map_err(|e| format!("{e:#}"))?;
    let _ = app.emit("config-changed", serde_json::json!({"source": "ipc"}));
    Ok(())
}

/// Pure implementation of `set_settings` callable without `tauri::State`.
pub fn set_settings_impl(
    engine: &Handle,
    config_path: &Path,
    s: Settings,
) -> anyhow::Result<()> {
    let mut doc = ConfigDoc::load(config_path)?;
    doc.set_deadzone(s.deadzone);
    doc.set_trigger_threshold(s.trigger_threshold);
    doc.set_min_press_ms_unchecked(s.min_press_ms);
    doc.set_tick_jitter_ms_unchecked(s.tick_jitter_ms);
    doc.set_log_events(s.log_events);
    doc.set_touchpad_cursor_enabled(s.touchpad_cursor_enabled);
    doc.set_touchpad_cursor_sensitivity(s.touchpad_cursor_sensitivity);
    doc.set_touchpad_midpoint_x(s.touchpad_midpoint_x);
    doc.set_touchpad_midpoint_y(s.touchpad_midpoint_y);
    doc.set_touchpad_accel_slow_threshold(s.touchpad_accel_slow_threshold);
    doc.set_touchpad_accel_fast_threshold(s.touchpad_accel_fast_threshold);
    doc.set_touchpad_accel_gain_slow(s.touchpad_accel_gain_slow);
    doc.set_touchpad_accel_gain_fast(s.touchpad_accel_gain_fast);
    doc.set_touchpad_deadzone_radius(s.touchpad_deadzone_radius);
    doc.set_touchpad_click_freeze_enabled(s.touchpad_click_freeze_enabled);
    doc.validate()?;
    write_atomic(config_path, &doc)?;
    let mut live = engine.config_write();
    live.deadzone = s.deadzone;
    live.trigger_threshold = s.trigger_threshold;
    live.min_press_ms = s.min_press_ms;
    live.tick_jitter_ms = s.tick_jitter_ms;
    live.log_events = s.log_events;
    live.touchpad_cursor_enabled = s.touchpad_cursor_enabled;
    live.touchpad_cursor_sensitivity = s.touchpad_cursor_sensitivity;
    live.touchpad_midpoint_x = s.touchpad_midpoint_x;
    live.touchpad_midpoint_y = s.touchpad_midpoint_y;
    live.touchpad_accel_slow_threshold = s.touchpad_accel_slow_threshold;
    live.touchpad_accel_fast_threshold = s.touchpad_accel_fast_threshold;
    live.touchpad_accel_gain_slow = s.touchpad_accel_gain_slow;
    live.touchpad_accel_gain_fast = s.touchpad_accel_gain_fast;
    live.touchpad_deadzone_radius = s.touchpad_deadzone_radius;
    live.touchpad_click_freeze_enabled = s.touchpad_click_freeze_enabled;
    drop(live);
    // Update the HID worker's atomics so cursor / quadrant changes take
    // effect on the next decoded frame without an engine restart.
    let params = engine.cursor_params();
    params.set_enabled(s.touchpad_cursor_enabled);
    params.set_sensitivity(s.touchpad_cursor_sensitivity);
    params.set_midpoint_x(s.touchpad_midpoint_x);
    params.set_midpoint_y(s.touchpad_midpoint_y);
    params.set_accel_slow_threshold(s.touchpad_accel_slow_threshold);
    params.set_accel_fast_threshold(s.touchpad_accel_fast_threshold);
    params.set_accel_gain_slow(s.touchpad_accel_gain_slow);
    params.set_accel_gain_fast(s.touchpad_accel_gain_fast);
    params.set_deadzone_radius(s.touchpad_deadzone_radius);
    params.set_click_freeze_enabled(s.touchpad_click_freeze_enabled);
    Ok(())
}

/// Tauri command: reset all settings to factory defaults (Spec §9).
#[cfg_attr(feature = "gui", tauri::command)]
#[cfg(feature = "gui")]
pub fn reset_settings(
    app: tauri::AppHandle,
    engine: State<'_, Handle>,
    config_path: State<'_, PathBuf>,
) -> Result<(), String> {
    reset_settings_impl(&*engine, &config_path).map_err(|e| format!("{e:#}"))?;
    let _ = app.emit("config-changed", serde_json::json!({"source": "ipc"}));
    Ok(())
}

/// Pure implementation of `reset_settings` callable without `tauri::State`.
pub fn reset_settings_impl(
    engine: &Handle,
    config_path: &Path,
) -> anyhow::Result<()> {
    set_settings_impl(engine, config_path, Settings::defaults())
}

// ─── pause_mapper ─────────────────────────────────────────────────────────────

/// Tauri command: pause or unpause the mapper engine.
///
/// When paused the engine still receives gamepad events but suppresses all
/// key synthesis. Already-held keys are released immediately on pause
/// (Iron rule #2 — `set_paused` calls `release_all_held`).
#[cfg(feature = "gui")]
#[tauri::command]
pub fn pause_mapper(
    engine: tauri::State<'_, crate::engine::Handle>,
    paused: bool,
) -> Result<(), String> {
    engine.set_paused(paused);
    Ok(())
}

// ─── open_config_in_editor ───────────────────────────────────────────────────

/// Tauri command: open the config file in the platform default text editor.
///
/// Windows: Notepad. macOS: default text editor via `open -t`. Linux: xdg-open.
/// The command is fire-and-forget; errors are surfaced as IPC error strings.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn open_config_in_editor(
    config_path: tauri::State<'_, std::path::PathBuf>,
) -> Result<(), String> {
    open_path(&*config_path).map_err(|e| format!("{e:#}"))
}

#[cfg(feature = "gui")]
fn open_path(path: &std::path::Path) -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("notepad")
            .arg(path)
            .spawn()
            .map_err(|e| anyhow::anyhow!("launching notepad: {e}"))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-t")
            .arg(path)
            .spawn()
            .map_err(|e| anyhow::anyhow!("launching open: {e}"))?;
    }
    #[cfg(all(target_os = "linux", feature = "gui"))]
    {
        // GUI is Windows-only in v0.2.0 but the lib still compiles on Linux;
        // use xdg-open as a courtesy.
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(|e| anyhow::anyhow!("launching xdg-open: {e}"))?;
    }
    Ok(())
}

// ─── quit ─────────────────────────────────────────────────────────────────────

/// Tauri command: exit the process cleanly via Tauri's event loop.
///
/// `app.exit(0)` causes `.run(...)` in `runtime::run` to return, after which
/// `engine.shutdown()` fires and `KeyboardSink::drop` releases held keys
/// (Iron rule #3). This IPC wrapper exists for frontend use (e.g. Settings
/// tab About → Quit button).
#[cfg(feature = "gui")]
#[tauri::command]
pub fn quit(app: tauri::AppHandle) -> Result<(), String> {
    app.exit(0);
    Ok(())
}

/// Tauri command: toggle the engine's capture-active flag.
///
/// Spec §11.2: while the frontend's key-capture surface is focused (bind
/// popup Key segment, macro step Key field), it must not pick up
/// self-synthesised keys. The frontend flips this flag around the capture
/// window; the engine pauses synth while it's set but still emits events.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn set_capture_active(
    engine: State<'_, Handle>,
    active: bool,
) -> Result<(), String> {
    engine.set_capture_active(active);
    Ok(())
}

/// Tauri command: turn the controller off. Signals the HID worker to drop
/// its current device handle, then powers the pad off at the Bluetooth-link
/// layer (`crate::platform::power_off_connected_dualsense`) — the DualSense
/// HID protocol has no power-off command, so this drops the Bluetooth bond
/// and the pad powers itself down. The worker thread stays alive and
/// returns to Searching; the pad must be re-paired to reconnect. The
/// frontend's "Turn Off" button in the toolbar invokes this.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn disconnect_gamepad(
    engine: State<'_, Handle>,
) -> Result<(), String> {
    engine.disconnect_current_device();
    Ok(())
}

/// Tauri command: return the current controller connection state.
///
/// The engine emits `controller-status` events on connect/disconnect, but
/// Tauri events don't replay missed emissions — a pad already paired before
/// the frontend's `listen()` registers is invisible to the UI otherwise.
/// Frontend polls this once on init to seed the status indicator.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn get_controller_status(
    engine: State<'_, Handle>,
) -> Result<Option<crate::engine::ControllerStatus>, String> {
    Ok(engine.current_status())
}

// ─── UiPrefs ──────────────────────────────────────────────────────────────────

/// Tauri command: load UI preferences from disk (drawer state, last tab, etc.).
///
/// Returns default prefs if the file is missing or corrupt — never an error.
/// Spec §11: ui-prefs live at `<exe-dir>/dualsense-mapper.ui.json`.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn get_ui_prefs(
    config_path: State<'_, PathBuf>,
) -> Result<crate::gui::ui_prefs::UiPrefs, String> {
    Ok(crate::gui::ui_prefs::load(
        &crate::gui::ui_prefs::path_beside(&config_path),
    ))
}

/// Tauri command: persist UI preferences to disk.
///
/// Called only on drawer toggle (~once per minute at most); synchronous write
/// is fine. Errors are surfaced as IPC error strings but the UI continues.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn set_ui_prefs(
    config_path: State<'_, PathBuf>,
    prefs: crate::gui::ui_prefs::UiPrefs,
) -> Result<(), String> {
    crate::gui::ui_prefs::save(
        &crate::gui::ui_prefs::path_beside(&config_path),
        &prefs,
    )
    .map_err(|e| format!("{e:#}"))
}

/// Tauri command: return the build-time `CARGO_PKG_VERSION`.
///
/// Iron rule #10 (v1.2.0): the Settings → About box must read the
/// Cargo version at runtime. Hardcoding a string in `settings.js` is
/// how v1.0.4–v1.1.4 shipped with stale "v0.2.0" displayed under the
/// version bumps.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn get_app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Tauri command: open a URL in the OS default browser.
///
/// The webview itself does not navigate `<a target="_blank">` links —
/// CSP `default-src 'self'` blocks it and Tauri 2 does not ship a
/// shell-open default in `withGlobalTauri`. Frontend intercepts anchor
/// clicks and calls this command. Only http:/https: URLs accepted to
/// avoid `file:`/`javascript:` smuggling.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(format!("refusing to open non-http(s) URL: {url}"));
    }
    open_external_url(&url).map_err(|e| format!("{e:#}"))
}

#[cfg(feature = "gui")]
fn open_external_url(url: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/c", "start", "", url])
            .spawn()
            .map_err(|e| anyhow::anyhow!("launching default browser: {e}"))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| anyhow::anyhow!("launching open: {e}"))?;
    }
    #[cfg(all(target_os = "linux", feature = "gui"))]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|e| anyhow::anyhow!("launching xdg-open: {e}"))?;
    }
    Ok(())
}
