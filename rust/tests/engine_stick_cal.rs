//! A saved calibration reaches the mapper; a reset takes effect on the next
//! stick event.

use dualsense_mapper::config::{Binding, ButtonEntry, Config};
use dualsense_mapper::engine::Engine;
use dualsense_mapper::gamepad::stick_cal::{AxisCal, PadStickCal, StickCal};
use dualsense_mapper::gamepad::{ControllerKind, GamepadEvent};
use dualsense_mapper::gui::commands::{reset_stick_cal_impl, save_stick_cal_impl};
use std::time::Duration;

const STEP: Duration = Duration::from_millis(100);

#[test]
fn reset_takes_effect_on_next_event() {
    let example = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config.example.json");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.json");
    let mut cfg = Config::load_from_path(&example).unwrap();
    cfg.deadzone = 0.8;
    cfg.bind_everywhere(18, ButtonEntry { label: "L-stick →".into(), binding: Binding::Key("d".into()) });
    std::fs::write(&path, serde_json::to_string_pretty(&cfg).unwrap()).unwrap();
    let engine = Engine::spawn_with_fake_gamepad(cfg).unwrap();
    let h = engine.handle();
    h.fake_event(GamepadEvent::Connected(ControllerKind::DualSense));
    std::thread::sleep(STEP);

    // lx: centre 0.1, +extent 0.6 → raw 0.7 reads as 1.0 (past the 0.8 deadzone).
    let a = AxisCal { center: 0.1, neg: 0.8, pos: 0.6 };
    let id = AxisCal::IDENTITY;
    save_stick_cal_impl(&h, &path, ControllerKind::DualSense,
        PadStickCal { left: StickCal { x: a, y: id }, right: StickCal { x: id, y: id } }).unwrap();
    std::thread::sleep(STEP);
    h.fake_event(GamepadEvent::Stick { axis: 0, value: 0.7 });
    std::thread::sleep(STEP);
    assert_eq!(h.held_keys_count(), 1, "corrected 1.0 presses L-stick →");
    let p = h.stick_positions().expect("probe");
    assert!((p.raw[0] - 0.7).abs() < 1e-6 && (p.cal[0] - 1.0).abs() < 1e-5, "{p:?}");

    reset_stick_cal_impl(&h, &path, ControllerKind::DualSense).unwrap();
    std::thread::sleep(STEP);
    h.fake_event(GamepadEvent::Stick { axis: 0, value: 0.69 });
    std::thread::sleep(STEP);
    assert_eq!(h.held_keys_count(), 0, "uncorrected 0.69 is inside the deadzone: released");
    engine.shutdown();
}
