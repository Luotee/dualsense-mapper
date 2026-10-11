//! A `Connected` makes the engine ask the session owner for one rumble
//! (config `rumble_on_connect`, default on), without ever blocking its tick.

#[allow(dead_code)] // only load_example() is used here
mod test_helpers;

use dualsense_mapper::engine::Engine;
use dualsense_mapper::gamepad::{ControllerKind, GamepadEvent};
use std::time::{Duration, Instant};

#[test]
fn connected_requests_one_rumble() {
    let engine = Engine::spawn_with_fake_gamepad(test_helpers::load_example()).expect("spawn");
    let h = engine.handle();
    h.fake_event(GamepadEvent::Connected(ControllerKind::DualSense));

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut rumbles = 0;
    while rumbles == 0 && Instant::now() < deadline {
        rumbles += h.fake_rumbles_received();
        std::thread::sleep(Duration::from_millis(5));
    }
    // Let a duplicate show up if there were one.
    std::thread::sleep(Duration::from_millis(200));
    rumbles += h.fake_rumbles_received();
    assert_eq!(rumbles, 1, "one Connected, one rumble request");
    assert_eq!(h.fake_disconnects_received(), 0, "a rumble is not a disconnect");

    engine.shutdown();
}

#[test]
fn no_rumble_when_disabled() {
    let mut cfg = test_helpers::load_example();
    cfg.rumble_on_connect = false;
    let engine = Engine::spawn_with_fake_gamepad(cfg).expect("spawn");
    let h = engine.handle();
    h.fake_event(GamepadEvent::Connected(ControllerKind::DualSense));
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(h.fake_rumbles_received(), 0);

    engine.shutdown();
}

#[test]
fn engine_does_not_block_on_a_full_command_channel() {
    let engine = Engine::spawn_with_fake_gamepad(test_helpers::load_example()).expect("spawn");
    let h = engine.handle();
    // 20 rumble requests into a bounded(4) channel, without waiting.
    for _ in 0..20 {
        h.fake_event(GamepadEvent::Connected(ControllerKind::DualSense));
        h.fake_event(GamepadEvent::Disconnected);
    }
    h.fake_event(GamepadEvent::Connected(ControllerKind::NsPro));

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut kind = None;
    while Instant::now() < deadline {
        kind = h.current_status().map(|s| s.kind);
        if kind == Some(ControllerKind::NsPro) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(kind, Some(ControllerKind::NsPro), "engine thread kept running");

    engine.shutdown();
}
