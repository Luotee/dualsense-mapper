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

#[test]
fn delete_macro_blocked_when_bound_to_button() {
    let mut cfg = test_helpers::load_example();
    // Ensure macro "doomed" exists and is bound to button id 23 (L2 trigger).
    cfg.macros.insert("doomed".into(), sample_macro());
    cfg.bind_everywhere(23,
        ButtonEntry {
            label: "L2".into(),
            binding: Binding::Macro("doomed".into()),
        },
    );
    let cfg_path = test_helpers::tmp_config_with(&cfg);
    let engine = Engine::spawn_with_fake_gamepad(cfg.clone()).unwrap();
    let handle = engine.handle();

    let err = delete_macro_impl(&handle, cfg_path.path(), "doomed").unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("doomed"), "error names macro; got: {msg}");
    assert!(
        msg.contains("23") || msg.to_lowercase().contains("bound"),
        "error names binding location; got: {msg}"
    );

    // Macro still on disk + in live config
    assert!(handle.config_read().macros.contains_key("doomed"));
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
