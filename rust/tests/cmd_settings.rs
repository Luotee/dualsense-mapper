mod test_helpers;

use dualsense_mapper::engine::Engine;
use dualsense_mapper::gui::commands::{Settings, set_settings_impl, reset_settings_impl};

#[test]
fn set_settings_persists_and_rejects_invalid_ranges() {
    let cfg = test_helpers::load_example();
    let cfg_path = test_helpers::tmp_config_with(&cfg);
    let engine = Engine::spawn_with_fake_gamepad(cfg.clone()).unwrap();
    let handle = engine.handle();

    // Invalid: min >= max.
    let err = set_settings_impl(&handle, cfg_path.path(), Settings {
        deadzone: 0.4,
        trigger_threshold: 0.5,
        min_press_ms: [25, 8],
        tick_jitter_ms: [0, 3],
        log_events: true,
        touchpad_cursor_enabled: true,
        touchpad_cursor_sensitivity: 1.5,
        touchpad_midpoint_x: 960,
        touchpad_midpoint_y: 540,
        ..Settings::defaults()
    }).unwrap_err();
    assert!(format!("{err:#}").to_lowercase().contains("min_press_ms"),
            "error should name the bad field; got: {err:#}");

    // Valid: deadzone change.
    set_settings_impl(&handle, cfg_path.path(), Settings {
        deadzone: 0.6,
        trigger_threshold: 0.5,
        min_press_ms: [8, 25],
        tick_jitter_ms: [0, 3],
        log_events: true,
        touchpad_cursor_enabled: true,
        touchpad_cursor_sensitivity: 1.5,
        touchpad_midpoint_x: 960,
        touchpad_midpoint_y: 540,
        ..Settings::defaults()
    }).unwrap();

    let live = handle.config_read();
    assert!((live.deadzone - 0.6).abs() < 1e-4, "deadzone hot-rebound; got {}", live.deadzone);
    drop(live);
    engine.shutdown();
}

#[test]
fn set_settings_pushes_touchpad_cursor_to_hid_atomics() {
    let cfg = test_helpers::load_example();
    let cfg_path = test_helpers::tmp_config_with(&cfg);
    let engine = Engine::spawn_with_fake_gamepad(cfg.clone()).unwrap();
    let handle = engine.handle();

    set_settings_impl(&handle, cfg_path.path(), Settings {
        deadzone: 0.4,
        trigger_threshold: 0.5,
        min_press_ms: [8, 25],
        tick_jitter_ms: [0, 3],
        log_events: true,
        touchpad_cursor_enabled: false,
        touchpad_cursor_sensitivity: 3.25,
        touchpad_midpoint_x: 850,
        touchpad_midpoint_y: 480,
        ..Settings::defaults()
    }).unwrap();

    let params = handle.cursor_params();
    assert!(!params.enabled());
    assert!((params.sensitivity() - 3.25).abs() < 1e-4,
        "live sensitivity hot-rebound; got {}", params.sensitivity());
    assert_eq!(params.midpoint_x(), 850);
    assert_eq!(params.midpoint_y(), 480);
    engine.shutdown();
}

#[test]
fn set_settings_rejects_sensitivity_out_of_range() {
    let cfg = test_helpers::load_example();
    let cfg_path = test_helpers::tmp_config_with(&cfg);
    let engine = Engine::spawn_with_fake_gamepad(cfg.clone()).unwrap();
    let handle = engine.handle();

    let err = set_settings_impl(&handle, cfg_path.path(), Settings {
        deadzone: 0.4,
        trigger_threshold: 0.5,
        min_press_ms: [8, 25],
        tick_jitter_ms: [0, 3],
        log_events: true,
        touchpad_cursor_enabled: true,
        touchpad_cursor_sensitivity: 99.0,
        touchpad_midpoint_x: 960,
        touchpad_midpoint_y: 540,
        ..Settings::defaults()
    }).unwrap_err();
    assert!(format!("{err:#}").contains("touchpad_cursor_sensitivity"),
        "expected the bad field name in the error; got: {err:#}");
    engine.shutdown();
}

#[test]
fn reset_settings_writes_factory_defaults() {
    let mut cfg = test_helpers::load_example();
    cfg.deadzone = 0.9;
    cfg.trigger_threshold = 0.9;
    cfg.min_press_ms = [40, 60];
    cfg.tick_jitter_ms = [5, 10];
    cfg.log_events = false;
    let cfg_path = test_helpers::tmp_config_with(&cfg);
    let engine = Engine::spawn_with_fake_gamepad(cfg.clone()).unwrap();
    let handle = engine.handle();

    reset_settings_impl(&handle, cfg_path.path()).unwrap();

    let live = handle.config_read();
    assert!((live.deadzone - 0.4).abs() < 1e-4, "deadzone reset; got {}", live.deadzone);
    assert!((live.trigger_threshold - 0.5).abs() < 1e-4);
    assert_eq!(live.min_press_ms, [8, 25]);
    assert_eq!(live.tick_jitter_ms, [0, 3]);
    assert_eq!(live.log_events, true);
    drop(live);
    engine.shutdown();
}

#[test]
fn set_rumble_on_connect_persists_and_reaches_engine() {
    use dualsense_mapper::gui::commands::set_rumble_on_connect_impl;
    let example = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config.example.json");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.json");
    std::fs::copy(&example, &path).unwrap();
    let cfg = dualsense_mapper::config::Config::load_from_path(&path).unwrap();
    let engine = Engine::spawn_with_fake_gamepad(cfg).unwrap();
    let h = engine.handle();
    let help = |p: &std::path::Path| {
        serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(p).unwrap()).unwrap()["_help"].clone()
    };
    let before = help(&path);
    set_rumble_on_connect_impl(&h, &path, false).unwrap();
    let disk: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(disk["rumble_on_connect"], false);
    assert_eq!(help(&path), before);
    assert!(!h.config_read().rumble_on_connect);
    set_rumble_on_connect_impl(&h, &path, true).unwrap();
    assert!(h.config_read().rumble_on_connect);
    engine.shutdown();
}
