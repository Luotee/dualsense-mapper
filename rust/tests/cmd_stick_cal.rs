//! save_stick_cal / reset_stick_cal: validated, persisted, live.

use dualsense_mapper::config::Config;
use dualsense_mapper::engine::Engine;
use dualsense_mapper::gamepad::stick_cal::{AxisCal, PadStickCal, StickCal};
use dualsense_mapper::gamepad::ControllerKind;
use dualsense_mapper::gui::commands::{reset_stick_cal_impl, save_stick_cal_impl};

fn cal(center: f32, pos: f32) -> PadStickCal {
    let a = AxisCal { center, neg: 0.9, pos };
    PadStickCal { left: StickCal { x: a, y: AxisCal::IDENTITY }, right: StickCal { x: AxisCal::IDENTITY, y: AxisCal::IDENTITY } }
}

#[test]
fn save_validates_and_persists() {
    let example = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config.example.json");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.json");
    std::fs::copy(&example, &path).unwrap();
    let engine = Engine::spawn_with_fake_gamepad(Config::load_from_path(&path).unwrap()).unwrap();
    let h = engine.handle();

    let before = std::fs::read_to_string(&path).unwrap();
    let err = save_stick_cal_impl(&h, &path, ControllerKind::NsPro, cal(0.4, 0.9)).unwrap_err();
    assert!(format!("{err:#}").contains("中心"), "{err:#}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before, "disk unchanged");

    save_stick_cal_impl(&h, &path, ControllerKind::NsPro, cal(0.05, 0.95)).unwrap();
    assert_eq!(Config::load_from_path(&path).unwrap().stick_cal["ns_pro"], cal(0.05, 0.95));
    assert_eq!(h.config_read().stick_cal["ns_pro"], cal(0.05, 0.95));

    reset_stick_cal_impl(&h, &path, ControllerKind::NsPro).unwrap();
    assert!(Config::load_from_path(&path).unwrap().stick_cal.is_empty());
    assert!(h.config_read().stick_cal.is_empty());
    engine.shutdown();
}
