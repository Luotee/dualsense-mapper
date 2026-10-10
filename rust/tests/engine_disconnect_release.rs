//! Controller disconnect (battery dies, out of range, Turn Off) while a
//! button is held: the engine's `Disconnected` handler must release every
//! held key and mouse button and cancel running macros (iron rules 2–4),
//! for every controller kind. A stick held at disconnect must not leave
//! stale virtual-button state that suppresses or duplicates a press after
//! reconnect.

#[allow(dead_code)] // only load_example() is used here
mod test_helpers;

use dualsense_mapper::config::{
    Binding, ButtonEntry, Config, MacroDef, MacroStep, MouseButton, StepAction,
};
use dualsense_mapper::engine::{Engine, Handle};
use dualsense_mapper::gamepad::{ControllerKind, GamepadEvent};
use std::time::Duration;

const STEP: Duration = Duration::from_millis(100);

/// Example config with mapping explicitly ON for the DualSense.
fn dualsense_cfg() -> Config {
    let mut cfg = test_helpers::load_example();
    cfg.mapping_enabled.insert("dualsense".into(), true);
    cfg
}

fn connect(h: &Handle, kind: ControllerKind) {
    h.fake_event(GamepadEvent::Connected(kind));
    std::thread::sleep(STEP);
}

fn disconnect(h: &Handle) {
    h.fake_event(GamepadEvent::Disconnected);
    std::thread::sleep(STEP);
}

#[test]
fn disconnect_while_zl_held_releases() {
    let mut cfg = test_helpers::load_example();
    cfg.buttons.insert(
        "23".into(),
        ButtonEntry { label: "ZL".into(), binding: Binding::Key("a".into()) },
    );
    let engine = Engine::spawn_with_fake_gamepad(cfg).expect("spawn");
    let h = engine.handle();

    // NsPro defaults to mapping ON.
    connect(&h, ControllerKind::NsPro);

    // What the HID worker emits for a ZL press on a Switch Pro.
    h.fake_event(GamepadEvent::ButtonDown(23));
    h.fake_event(GamepadEvent::Trigger { axis: 4, value: 1.0 });
    std::thread::sleep(STEP);
    assert!(h.held_keys_count() >= 1, "ZL press must hold its key");

    disconnect(&h);
    assert_eq!(h.held_keys_count(), 0, "Disconnected must release every held key");

    engine.shutdown();
}

#[test]
fn disconnect_while_key_held_releases_dualsense() {
    let engine = Engine::spawn_with_fake_gamepad(dualsense_cfg()).expect("spawn");
    let h = engine.handle();
    connect(&h, ControllerKind::DualSense);

    // Button 0 is bound to a key in maple_artale.json.
    h.fake_button_down(0);
    std::thread::sleep(STEP);
    assert_eq!(h.held_keys_count(), 1, "press registered");

    disconnect(&h);
    assert_eq!(h.held_keys_count(), 0, "Disconnected must release the held key");

    engine.shutdown();
}

#[test]
fn disconnect_cancels_running_macro() {
    let mut cfg = dualsense_cfg();
    cfg.macros.insert(
        "hold_left".into(),
        MacroDef {
            // repeat: true so the test fails if Disconnected only releases
            // held keys without stopping the macro (it would keep re-pressing).
            repeat: true,
            steps: vec![
                MacroStep { key: "Left".into(), action: StepAction::Down, delay_ms: [500, 600] },
                MacroStep { key: "Left".into(), action: StepAction::Up, delay_ms: [15, 25] },
            ],
        },
    );
    cfg.buttons.insert(
        "1".into(),
        ButtonEntry { label: "Circle".into(), binding: Binding::Macro("hold_left".into()) },
    );
    let engine = Engine::spawn_with_fake_gamepad(cfg).expect("spawn");
    let h = engine.handle();
    connect(&h, ControllerKind::DualSense);

    h.fake_button_down(1);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(h.held_keys_count(), 1, "macro pressed Left");

    disconnect(&h);
    assert_eq!(h.held_keys_count(), 0, "Disconnected must cancel the macro and release Left");

    std::thread::sleep(Duration::from_millis(700));
    assert_eq!(h.held_keys_count(), 0, "cancelled macro must not re-press anything");

    engine.shutdown();
}

#[test]
fn disconnect_releases_held_mouse_button() {
    let mut cfg = dualsense_cfg();
    cfg.buttons.insert(
        "25".into(),
        ButtonEntry { label: "Touchpad TL".into(), binding: Binding::Mouse(MouseButton::Left) },
    );
    let engine = Engine::spawn_with_fake_gamepad(cfg).expect("spawn");
    let h = engine.handle();
    connect(&h, ControllerKind::DualSense);

    h.fake_button_down(25);
    std::thread::sleep(STEP);
    let held = |h: &Handle| h.key_state().lock().unwrap_or_else(|p| p.into_inner()).len_held_mouse();
    assert_eq!(held(&h), 1, "mouse button held");

    disconnect(&h);
    assert_eq!(held(&h), 0, "Disconnected must release the held mouse button");

    engine.shutdown();
}

#[test]
fn reconnect_after_disconnect_with_stick_held_no_stuck_key() {
    // id 15 = L-stick up (axis 1, positive) → "Up" in maple_artale.json.
    let engine = Engine::spawn_with_fake_gamepad(dualsense_cfg()).expect("spawn");
    let h = engine.handle();
    connect(&h, ControllerKind::DualSense);

    h.fake_event(GamepadEvent::Stick { axis: 1, value: 1.0 });
    std::thread::sleep(STEP);
    assert_eq!(h.held_keys_count(), 1, "stick virtual key held");

    disconnect(&h);
    assert_eq!(h.held_keys_count(), 0, "Disconnected must release the stick virtual key");

    connect(&h, ControllerKind::DualSense);
    h.fake_event(GamepadEvent::Stick { axis: 1, value: 0.0 });
    std::thread::sleep(STEP);
    assert_eq!(h.held_keys_count(), 0, "neutral stick after reconnect holds nothing");

    h.fake_event(GamepadEvent::Stick { axis: 1, value: 1.0 });
    std::thread::sleep(STEP);
    assert_eq!(h.held_keys_count(), 1, "stick up after reconnect presses again");

    engine.shutdown();
}

#[test]
fn reconnect_with_stick_still_held_presses_again() {
    // The HID worker's first frame of a new session reports every axis
    // (no previous state). If the stick is still up at reconnect, that
    // first report must press the key again — stale virtual state from the
    // previous session must not swallow it.
    let engine = Engine::spawn_with_fake_gamepad(dualsense_cfg()).expect("spawn");
    let h = engine.handle();
    connect(&h, ControllerKind::DualSense);

    h.fake_event(GamepadEvent::Stick { axis: 1, value: 1.0 });
    std::thread::sleep(STEP);
    assert_eq!(h.held_keys_count(), 1, "stick virtual key held");

    disconnect(&h);
    assert_eq!(h.held_keys_count(), 0, "Disconnected must release the stick virtual key");

    connect(&h, ControllerKind::DualSense);
    h.fake_event(GamepadEvent::Stick { axis: 1, value: 1.0 });
    std::thread::sleep(STEP);
    assert_eq!(h.held_keys_count(), 1, "stick still up at reconnect must press again");

    h.fake_event(GamepadEvent::Stick { axis: 1, value: 0.0 });
    std::thread::sleep(STEP);
    assert_eq!(h.held_keys_count(), 0, "returning to neutral releases it");

    engine.shutdown();
}

/// The tray "pause" leaves Turn Off clickable: the request must reach the
/// owning source while the engine is paused (it skips `poll` then), not
/// queue until unpause.
#[test]
fn disconnect_while_paused_reaches_source() {
    let engine = Engine::spawn_with_fake_gamepad(dualsense_cfg()).expect("spawn");
    let h = engine.handle();
    connect(&h, ControllerKind::DualSense);
    h.set_paused(true);
    std::thread::sleep(STEP);

    h.disconnect_current_device();
    let deadline = std::time::Instant::now() + STEP;
    let mut got = 0;
    while got == 0 && std::time::Instant::now() < deadline {
        got += h.fake_disconnects_received();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(got, 1, "manual disconnect must reach the source while paused");

    engine.shutdown();
}
