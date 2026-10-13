mod test_helpers;

use dualsense_mapper::config::{Binding, ButtonEntry, MacroDef, MacroStep, StepAction};
use dualsense_mapper::engine::Engine;
use dualsense_mapper::gui::commands::{delete_macro_impl, rename_macro_impl, set_macro_impl};

fn sample_macro() -> MacroDef {
    MacroDef {
        repeat: true,
        steps: vec![
            MacroStep {
                key: "Left".into(),
                action: StepAction::Down,
                delay_ms: [50, 80],
            },
            MacroStep {
                key: "Left".into(),
                action: StepAction::Up,
                delay_ms: [15, 25],
            },
        ],
    }
}

#[test]
fn set_macro_creates_new() {
    let cfg = test_helpers::load_example();
    let cfg_path = test_helpers::tmp_config_with(&cfg);
    let engine = Engine::spawn_with_fake_gamepad(cfg.clone()).unwrap();
    let handle = engine.handle();

    set_macro_impl(&handle, cfg_path.path(), "shiny_new".into(), sample_macro()).unwrap();

    let live = handle.config_read();
    assert!(live.macros.contains_key("shiny_new"));
    drop(live);

    let on_disk = std::fs::read_to_string(cfg_path.path()).unwrap();
    assert!(on_disk.contains("\"shiny_new\""), "macro should appear on disk");
    engine.shutdown();
}

/// Bind `name` on dualsense base 3 and ns_pro quick_l 0 (on the config
/// that goes to disk and to the engine).
fn bind_on_two_pads(cfg: &mut dualsense_mapper::config::Config, name: &str) {
    use dualsense_mapper::gamepad::ControllerKind;
    let m = || ButtonEntry { label: "slot".into(), binding: Binding::Macro(name.into()) };
    cfg.pad_mut(ControllerKind::DualSense).buttons.insert("3".into(), m());
    cfg.pad_mut(ControllerKind::NsPro).quick_l.insert("0".into(), m());
}

#[test]
fn delete_macro_unbinds_everywhere() {
    use dualsense_mapper::gamepad::ControllerKind;
    let mut cfg = test_helpers::load_example();
    cfg.macros.insert("doomed".into(), sample_macro());
    bind_on_two_pads(&mut cfg, "doomed");
    let cfg_path = test_helpers::tmp_config_with(&cfg);
    let engine = Engine::spawn_with_fake_gamepad(cfg.clone()).unwrap();
    let handle = engine.handle();

    delete_macro_impl(&handle, cfg_path.path(), "doomed").unwrap();

    let disk = dualsense_mapper::config::Config::load_from_path(cfg_path.path()).unwrap();
    for c in [disk, handle.config_read().clone()] {
        assert!(!c.macros.contains_key("doomed"));
        assert_eq!(c.pad(ControllerKind::DualSense).buttons["3"].binding, Binding::Unbound);
        assert_eq!(c.pad(ControllerKind::DualSense).buttons["3"].label, "slot", "label kept");
        assert!(!c.pad(ControllerKind::NsPro).quick_l.contains_key("0"), "page entry removed");
    }
    engine.shutdown();
}

#[test]
fn delete_macro_succeeds_when_unbound() {
    let mut cfg = test_helpers::load_example();
    cfg.macros.insert("orphan".into(), sample_macro());
    let cfg_path = test_helpers::tmp_config_with(&cfg);
    let engine = Engine::spawn_with_fake_gamepad(cfg.clone()).unwrap();
    let handle = engine.handle();

    delete_macro_impl(&handle, cfg_path.path(), "orphan").unwrap();

    assert!(!handle.config_read().macros.contains_key("orphan"));
    let on_disk = std::fs::read_to_string(cfg_path.path()).unwrap();
    assert!(!on_disk.contains("\"orphan\""), "macro should be removed from disk");
    engine.shutdown();
}

#[test]
fn rename_macro_updates_all_bindings() {
    let mut cfg = test_helpers::load_example();
    cfg.macros.insert("old_name".into(), sample_macro());
    cfg.bind_everywhere(23,
        ButtonEntry {
            label: "L2".into(),
            binding: Binding::Macro("old_name".into()),
        },
    );
    let cfg_path = test_helpers::tmp_config_with(&cfg);
    let engine = Engine::spawn_with_fake_gamepad(cfg.clone()).unwrap();
    let handle = engine.handle();

    rename_macro_impl(&handle, cfg_path.path(), "old_name", "new_name").unwrap();

    let live = handle.config_read();
    assert!(!live.macros.contains_key("old_name"));
    assert!(live.macros.contains_key("new_name"));
    // Binding for button 23 follows the rename
    if let Binding::Macro(ref name) = live.pad(dualsense_mapper::gamepad::ControllerKind::DualSense).buttons["23"].binding {
        assert_eq!(name, "new_name");
    } else {
        panic!("button 23 should still be a macro binding after rename");
    }
    drop(live);
    engine.shutdown();
}

#[test]
fn rename_macro_rewrites_all_pads_and_pages() {
    use dualsense_mapper::gamepad::ControllerKind;
    let mut cfg = test_helpers::load_example();
    cfg.macros.insert("before".into(), sample_macro());
    bind_on_two_pads(&mut cfg, "before");
    let cfg_path = test_helpers::tmp_config_with(&cfg);
    let engine = Engine::spawn_with_fake_gamepad(cfg.clone()).unwrap();
    let handle = engine.handle();

    rename_macro_impl(&handle, cfg_path.path(), "before", "after").unwrap();

    let disk = dualsense_mapper::config::Config::load_from_path(cfg_path.path()).unwrap();
    let want = Binding::Macro("after".into());
    for c in [disk, handle.config_read().clone()] {
        assert!(c.macros.contains_key("after") && !c.macros.contains_key("before"));
        assert_eq!(c.pad(ControllerKind::DualSense).buttons["3"].binding, want);
        assert_eq!(c.pad(ControllerKind::NsPro).quick_l["0"].binding, want);
    }
    engine.shutdown();
}
