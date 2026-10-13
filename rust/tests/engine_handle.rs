use dualsense_mapper::engine::{Engine, EngineEvent};
use dualsense_mapper::config::Config;
use std::time::Duration;

fn load_minimal_config() -> Config {
    let json = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("examples/maple_artale.json"),
    )
    .expect("example config must exist");
    Config::load_from_str(&json).expect("example config must parse")
}

#[test]
fn pause_drains_held_and_skips_subsequent_synth() {
    let cfg = load_minimal_config();
    let engine = Engine::spawn_with_fake_gamepad(cfg).expect("spawn");
    let handle = engine.handle();

    // Button 0 is bound to "x" in maple_artale.json.
    handle.fake_button_down(0);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(handle.held_keys_count(), 1, "press registered");

    handle.set_paused(true);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(handle.held_keys_count(), 0, "pause drained held keys");

    handle.fake_button_down(0); // event arrives while paused
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(handle.held_keys_count(), 0, "paused engine ignores events");

    handle.set_paused(false);
    handle.fake_button_down(0);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(handle.held_keys_count(), 1, "unpause restores synth");

    engine.shutdown();
}

#[test]
fn capture_active_gates_synth_only_not_event_emission() {
    let cfg = load_minimal_config();
    let engine = Engine::spawn_with_fake_gamepad(cfg).expect("spawn");
    let handle = engine.handle();

    handle.set_capture_active(true);
    handle.fake_button_down(0);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(handle.held_keys_count(), 0, "capture-active blocks synth");
    let events: Vec<_> = handle.drain_events();
    assert!(
        events.iter().any(|e| matches!(e, EngineEvent::ButtonDown { id: 0 })),
        "events still emit while capture-active; got: {events:?}"
    );

    engine.shutdown();
}

#[test]
fn hot_rebind_picks_up_new_binding_on_next_event() {
    let cfg = load_minimal_config();
    let engine = Engine::spawn_with_fake_gamepad(cfg).expect("spawn");
    let handle = engine.handle();

    // Swap binding for id 0 from "x" to Unbound.
    {
        let mut guard = handle.config_write();
        guard.pad_mut(dualsense_mapper::gamepad::ControllerKind::DualSense).buttons.get_mut("0").unwrap().binding = dualsense_mapper::config::Binding::Unbound;
    }

    // Give the engine loop a full tick (8 ms) to notice the config change.
    std::thread::sleep(Duration::from_millis(50));

    handle.fake_button_down(0);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(handle.held_keys_count(), 0, "Unbound after rebind");

    engine.shutdown();
}

#[test]
fn connected_status_carries_kind() {
    use dualsense_mapper::gamepad::{ControllerKind, GamepadEvent};
    let cfg = load_minimal_config();
    let engine = Engine::spawn_with_fake_gamepad(cfg).expect("spawn");
    let handle = engine.handle();

    handle.fake_event(GamepadEvent::Connected(ControllerKind::DualSense));
    std::thread::sleep(Duration::from_millis(100));

    let status = handle.current_status().expect("status after Connected");
    assert_eq!(status.kind, ControllerKind::DualSense);
    assert_eq!(status.name, "DualSense");

    engine.shutdown();
}
