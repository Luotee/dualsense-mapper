//! Per-controller mapping gate (`Config::mapping_enabled`): while mapping is
//! OFF for the connected controller the engine still emits button events but
//! synthesizes nothing, and every ON→OFF transition releases whatever was
//! held (iron rules 2–4: no stuck KEYDOWN, macros drained).

#[allow(dead_code)] // only load_example() is used here
mod test_helpers;

use dualsense_mapper::config::{Binding, ButtonEntry, Config, MacroDef, MacroStep, MouseButton, StepAction};
use dualsense_mapper::engine::{Engine, EngineEvent, Handle};
use dualsense_mapper::gamepad::{ControllerKind, GamepadEvent};
use std::time::Duration;

const STEP: Duration = Duration::from_millis(100);

fn config_with_dualsense(enabled: Option<bool>) -> Config {
    let mut cfg = test_helpers::load_example();
    if let Some(v) = enabled {
        cfg.mapping_enabled.insert("dualsense".into(), v);
    }
    cfg
}

fn connect_dualsense(h: &Handle) {
    h.fake_event(GamepadEvent::Connected(ControllerKind::DualSense));
    std::thread::sleep(STEP);
}

fn set_dualsense_mapping(h: &Handle, v: bool) {
    h.config_write().mapping_enabled.insert("dualsense".into(), v);
    std::thread::sleep(STEP);
}

#[test]
fn off_suppresses_synth_but_emits_button_events() {
    let engine = Engine::spawn_with_fake_gamepad(config_with_dualsense(Some(false))).expect("spawn");
    let h = engine.handle();
    connect_dualsense(&h);

    // Button 0 is bound to a key in maple_artale.json.
    h.fake_button_down(0);
    std::thread::sleep(STEP);
    assert_eq!(h.held_keys_count(), 0, "mapping OFF must not synthesize");
    let events = h.drain_events();
    assert!(
        events.iter().any(|e| matches!(e, EngineEvent::ButtonDown { id: 0 })),
        "button events still emit while mapping OFF; got: {events:?}"
    );

    engine.shutdown();
}

#[test]
fn toggle_off_mid_press_releases_held() {
    let engine = Engine::spawn_with_fake_gamepad(config_with_dualsense(Some(true))).expect("spawn");
    let h = engine.handle();
    connect_dualsense(&h);

    h.fake_button_down(0);
    std::thread::sleep(STEP);
    assert_eq!(h.held_keys_count(), 1, "press registered while ON");

    set_dualsense_mapping(&h, false);
    assert_eq!(h.held_keys_count(), 0, "ON→OFF must release held keys");

    engine.shutdown();
}

#[test]
fn toggle_off_releases_stick_virtual_key() {
    let mut cfg = config_with_dualsense(Some(true));
    // id 15 = L-stick up (axis 1, positive).
    cfg.bind_everywhere(15,
        ButtonEntry { label: "L-stick up".into(), binding: Binding::Key("Up".into()) },
    );
    let engine = Engine::spawn_with_fake_gamepad(cfg).expect("spawn");
    let h = engine.handle();
    connect_dualsense(&h);

    h.fake_event(GamepadEvent::Stick { axis: 1, value: 1.0 });
    std::thread::sleep(STEP);
    assert_eq!(h.held_keys_count(), 1, "stick virtual key held while ON");

    set_dualsense_mapping(&h, false);
    assert_eq!(h.held_keys_count(), 0, "ON→OFF must release stick virtual key");

    engine.shutdown();
}

#[test]
fn toggle_off_cancels_running_macro() {
    let mut cfg = config_with_dualsense(Some(true));
    cfg.macros.insert(
        "hold_left".into(),
        MacroDef {
            repeat: false,
            steps: vec![
                MacroStep { key: "Left".into(), action: StepAction::Down, delay_ms: [500, 600] },
                MacroStep { key: "Left".into(), action: StepAction::Up, delay_ms: [15, 25] },
            ],
        },
    );
    cfg.bind_everywhere(1,
        ButtonEntry { label: "Circle".into(), binding: Binding::Macro("hold_left".into()) },
    );
    let engine = Engine::spawn_with_fake_gamepad(cfg).expect("spawn");
    let h = engine.handle();
    connect_dualsense(&h);

    h.fake_button_down(1);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(h.held_keys_count(), 1, "macro pressed Left");

    set_dualsense_mapping(&h, false);
    assert_eq!(h.held_keys_count(), 0, "ON→OFF must cancel the macro and release Left");

    std::thread::sleep(Duration::from_millis(700));
    assert_eq!(h.held_keys_count(), 0, "cancelled macro must not re-press anything");

    engine.shutdown();
}

#[test]
fn toggle_on_with_button_held_no_stuck_key() {
    let engine = Engine::spawn_with_fake_gamepad(config_with_dualsense(Some(false))).expect("spawn");
    let h = engine.handle();
    connect_dualsense(&h);

    h.fake_button_down(0);
    std::thread::sleep(STEP);
    assert_eq!(h.held_keys_count(), 0, "mapping OFF must not synthesize");

    set_dualsense_mapping(&h, true);
    h.fake_button_up(0);
    std::thread::sleep(STEP);
    assert_eq!(h.held_keys_count(), 0, "release after OFF→ON must not strand a key");

    engine.shutdown();
}

#[test]
fn unpause_while_off_does_not_synth() {
    let engine = Engine::spawn_with_fake_gamepad(config_with_dualsense(Some(false))).expect("spawn");
    let h = engine.handle();
    connect_dualsense(&h);

    h.set_paused(true);
    std::thread::sleep(STEP);
    h.set_paused(false);
    std::thread::sleep(STEP);

    h.fake_button_down(0);
    std::thread::sleep(STEP);
    assert_eq!(h.held_keys_count(), 0, "unpause must not re-enable synth while mapping OFF");

    engine.shutdown();
}

#[test]
fn no_controller_connected_defaults_to_on() {
    let engine = Engine::spawn_with_fake_gamepad(config_with_dualsense(Some(false))).expect("spawn");
    let h = engine.handle();

    // No Connected event: the gate does not know the controller kind and
    // falls back to ON (matches every pre-existing engine test).
    h.fake_button_down(0);
    std::thread::sleep(STEP);
    assert_eq!(h.held_keys_count(), 1, "no controller kind known → mapping ON");

    engine.shutdown();
}

#[test]
fn connected_and_press_in_same_batch_do_not_synth_when_off() {
    let engine = Engine::spawn_with_fake_gamepad(config_with_dualsense(Some(false))).expect("spawn");
    let h = engine.handle();
    for _ in 0..20 {
        h.fake_event(GamepadEvent::Connected(ControllerKind::DualSense));
        h.fake_button_down(0);
        std::thread::sleep(Duration::from_millis(30));
        h.fake_button_up(0);
        h.fake_event(GamepadEvent::Disconnected);
        std::thread::sleep(Duration::from_millis(30));
    }
    let events = h.drain_events();
    assert!(
        !events.iter().any(|e| matches!(e, EngineEvent::KeyEmit { action: "down", .. })),
        "first-frame press must not synth while mapping OFF; got: {events:?}"
    );
    engine.shutdown();
}

#[test]
fn toggle_off_releases_held_mouse_button() {
    let mut cfg = config_with_dualsense(Some(true));
    cfg.bind_everywhere(25,
        ButtonEntry { label: "Touchpad TL".into(), binding: Binding::Mouse(MouseButton::Left) },
    );
    let engine = Engine::spawn_with_fake_gamepad(cfg).expect("spawn");
    let h = engine.handle();
    connect_dualsense(&h);

    h.fake_button_down(25);
    std::thread::sleep(STEP);
    let held = |h: &Handle| h.key_state().lock().unwrap_or_else(|p| p.into_inner()).len_held_mouse();
    assert_eq!(held(&h), 1, "mouse button held while ON");

    set_dualsense_mapping(&h, false);
    assert_eq!(held(&h), 0, "ON→OFF must release held mouse button");

    engine.shutdown();
}
