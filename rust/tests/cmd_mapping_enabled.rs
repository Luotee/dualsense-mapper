mod test_helpers;

use dualsense_mapper::engine::Engine;
use dualsense_mapper::gamepad::ControllerKind;
use dualsense_mapper::gui::commands::set_mapping_enabled_impl;

#[test]
fn persists_and_updates_live_engine() {
    let cfg = test_helpers::load_example();
    let cfg_path = test_helpers::tmp_config_with(&cfg);
    let engine = Engine::spawn_with_fake_gamepad(cfg.clone()).unwrap();
    let h = engine.handle();

    set_mapping_enabled_impl(&h, cfg_path.path(), "dualsense", false).unwrap();

    let disk: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(cfg_path.path()).unwrap()).unwrap();
    assert_eq!(disk["mapping_enabled"]["dualsense"], serde_json::json!(false));
    assert!(!h.config_read().mapping_enabled_for(ControllerKind::DualSense));
    engine.shutdown();
}

#[test]
fn rejects_unknown_kind_and_leaves_disk_untouched() {
    let cfg = test_helpers::load_example();
    let cfg_path = test_helpers::tmp_config_with(&cfg);
    let engine = Engine::spawn_with_fake_gamepad(cfg.clone()).unwrap();
    let h = engine.handle();

    let before = std::fs::read(cfg_path.path()).unwrap();
    let err = set_mapping_enabled_impl(&h, cfg_path.path(), "ps5", false).unwrap_err();
    assert!(format!("{err:#}").contains("ps5"), "error should name the kind; got: {err:#}");
    assert_eq!(before, std::fs::read(cfg_path.path()).unwrap());
    engine.shutdown();
}

#[test]
fn old_config_without_field_gains_it() {
    let cfg = test_helpers::load_example();
    let mut v = serde_json::to_value(&cfg).unwrap();
    v.as_object_mut().unwrap().remove("mapping_enabled");
    let f = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(f.path(), serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let engine = Engine::spawn_with_fake_gamepad(cfg.clone()).unwrap();
    let h = engine.handle();

    set_mapping_enabled_impl(&h, f.path(), "dualsense", false).unwrap();

    let disk: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(f.path()).unwrap()).unwrap();
    let map = disk["mapping_enabled"].as_object().expect("field gained");
    assert_eq!(map.len(), 1, "only dualsense present; got {map:?}");
    assert_eq!(map["dualsense"], serde_json::json!(false));
    engine.shutdown();
}
