mod test_helpers;

use dualsense_mapper::config::{Binding, ButtonEntry};
use dualsense_mapper::engine::Engine;

#[test]
fn set_binding_persists_and_updates_live_engine() {
    let cfg = test_helpers::load_example();
    let cfg_path = test_helpers::tmp_config_with(&cfg);
    let engine = Engine::spawn_with_fake_gamepad(cfg.clone()).unwrap();
    let handle = engine.handle();

    dualsense_mapper::gui::commands::set_binding_impl(
        &handle,
        cfg_path.path(),
        3,
        ButtonEntry { label: "Triangle".into(), binding: Binding::Key("F1".into()) },
    )
    .unwrap();

    // hot rebind: live config reflects the change
    let live = handle.config_read();
    assert!(
        matches!(live.pad(dualsense_mapper::gamepad::ControllerKind::DualSense).buttons.get("3").unwrap().binding, Binding::Key(ref k) if k == "F1"),
        "live config should have F1; got: {:?}",
        live.pad(dualsense_mapper::gamepad::ControllerKind::DualSense).buttons.get("3")
    );

    // persisted: disk was written
    let on_disk = std::fs::read_to_string(cfg_path.path()).unwrap();
    assert!(
        on_disk.contains("\"F1\""),
        "F1 should appear on disk; got: {}",
        &on_disk[..200.min(on_disk.len())]
    );
    drop(live);
    engine.shutdown();
}

#[test]
fn set_binding_rejects_invalid_key_name_and_does_not_persist() {
    let cfg = test_helpers::load_example();
    let cfg_path = test_helpers::tmp_config_with(&cfg);
    let pre = std::fs::read_to_string(cfg_path.path()).unwrap();
    let engine = Engine::spawn_with_fake_gamepad(cfg.clone()).unwrap();
    let handle = engine.handle();

    // "F99" is not a recognised key name (parse_key rejects)
    let err = dualsense_mapper::gui::commands::set_binding_impl(
        &handle,
        cfg_path.path(),
        3,
        ButtonEntry { label: "Triangle".into(), binding: Binding::Key("F99".into()) },
    )
    .unwrap_err();
    assert!(
        format!("{err:#}").to_lowercase().contains("f99")
            || format!("{err:#}").to_lowercase().contains("unknown")
            || format!("{err:#}").to_lowercase().contains("invalid"),
        "error should name the bad key; got: {err:#}"
    );

    // disk untouched
    let post = std::fs::read_to_string(cfg_path.path()).unwrap();
    assert_eq!(pre, post, "disk should not be written on validation failure");

    // live state unchanged (still the original binding for id 3)
    let live = handle.config_read();
    if let Binding::Key(ref k) = live.pad(dualsense_mapper::gamepad::ControllerKind::DualSense).buttons.get("3").unwrap().binding {
        assert_ne!(k, "F99");
    }
    drop(live);
    engine.shutdown();
}
