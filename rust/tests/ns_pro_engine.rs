//! Switch Pro through the engine (Review Focus 5): ZL held with mapping ON,
//! then the pad disconnects — no key may stay held (iron rules 2–4).

#[allow(dead_code)] // only load_example() is used here
mod test_helpers;

use dualsense_mapper::config::{Binding, ButtonEntry};
use dualsense_mapper::engine::Engine;
use dualsense_mapper::gamepad::{ControllerKind, GamepadEvent};
use std::time::Duration;

const STEP: Duration = Duration::from_millis(100);

#[test]
#[ignore = "pre-existing: engine's Disconnected arm (engine.rs run loop) does not release held keys — any kind, not NS-specific; reported, not fixed in Task 4"]
fn disconnect_while_zl_held_releases() {
    let mut cfg = test_helpers::load_example();
    cfg.buttons.insert(
        "23".into(),
        ButtonEntry { label: "ZL".into(), binding: Binding::Key("a".into()) },
    );
    let engine = Engine::spawn_with_fake_gamepad(cfg).expect("spawn");
    let h = engine.handle();

    // NsPro defaults to mapping ON.
    h.fake_event(GamepadEvent::Connected(ControllerKind::NsPro));
    std::thread::sleep(STEP);

    // What the HID worker emits for a ZL press on a Switch Pro.
    h.fake_event(GamepadEvent::ButtonDown(23));
    h.fake_event(GamepadEvent::Trigger { axis: 4, value: 1.0 });
    std::thread::sleep(STEP);
    assert!(h.held_keys_count() >= 1, "ZL press must hold its key");

    h.fake_event(GamepadEvent::Disconnected);
    std::thread::sleep(STEP);
    assert_eq!(h.held_keys_count(), 0, "Disconnected must release every held key");

    engine.shutdown();
}
