//! Quick pages and trigger modes through the running engine (fake gamepad).

use dualsense_mapper::config::{Binding, ButtonEntry, Config};
use dualsense_mapper::engine::{Engine, Handle};
use dualsense_mapper::gamepad::{ControllerKind, GamepadEvent};
use dualsense_mapper::pad_bindings::Page;
use std::time::Duration;

const STEP: Duration = Duration::from_millis(100);

fn send(h: &Handle, ev: GamepadEvent) {
    h.fake_event(ev);
    std::thread::sleep(STEP);
}

/// A v1 file with L2 (23) bound to macro `m`: one `Down` of F held for
/// 2–3 s. (`loop: false`: the macro releases F when its last step ends —
/// iron rule 4 — so the long delay keeps F held while the test looks.)
fn v1_l2_macro_json() -> String {
    let mut v: serde_json::Value = serde_json::from_str(
        r#"{
            "version": 1, "deadzone": 0.4, "trigger_threshold": 0.5,
            "min_press_ms": [8, 25], "tick_jitter_ms": [0, 3],
            "log_events": false, "buttons": {},
            "macros": { "m": { "loop": false, "steps": [
                { "key": "F", "action": "down", "delay_ms": [2000, 3000] }
            ] } }
        }"#,
    )
    .unwrap();
    for id in 0u32..=32 {
        v["buttons"][id.to_string()] = serde_json::json!({"label": "", "type": "unbound"});
    }
    v["buttons"]["23"] = serde_json::json!({"label": "L2", "type": "macro", "value": "m"});
    v.to_string()
}

#[test]
fn migrated_v1_l2_macro_still_fires() {
    let cfg = Config::load_from_str(&v1_l2_macro_json()).unwrap();
    let engine = Engine::spawn_with_fake_gamepad(cfg).unwrap();
    let h = engine.handle();
    send(&h, GamepadEvent::Connected(ControllerKind::DualSense));
    send(&h, GamepadEvent::Trigger { axis: 4, value: 1.0 });
    assert_eq!(h.held_keys_count(), 1, "L2 in key mode starts the macro, which holds F");
    send(&h, GamepadEvent::Trigger { axis: 4, value: 0.0 });
    assert_eq!(h.held_keys_count(), 0, "releasing L2 stops the macro and drains F");
    engine.shutdown();
}

#[test]
fn quick_page_press_via_engine() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config.example.json");
    let mut cfg = Config::load_from_path(&path).unwrap();
    let ns = cfg.pad_mut(ControllerKind::NsPro);
    ns.buttons.get_mut("11").unwrap().binding = Binding::Unbound;
    ns.table_mut(Page::QuickL)
        .insert("11".into(), ButtonEntry { label: String::new(), binding: Binding::Key("F1".into()) });
    cfg.validate().unwrap();
    let engine = Engine::spawn_with_fake_gamepad(cfg).unwrap();
    let h = engine.handle();

    send(&h, GamepadEvent::Connected(ControllerKind::NsPro));
    send(&h, GamepadEvent::Trigger { axis: 4, value: 1.0 });
    assert_eq!(h.held_keys_count(), 0, "a page-mode trigger sends nothing");
    send(&h, GamepadEvent::ButtonDown(11));
    assert_eq!(h.held_keys_count(), 1, "quick_l 11 = F1 held");
    send(&h, GamepadEvent::ButtonUp(11));
    assert_eq!(h.held_keys_count(), 0);

    send(&h, GamepadEvent::Trigger { axis: 4, value: 0.0 });
    send(&h, GamepadEvent::ButtonDown(11));
    assert_eq!(h.held_keys_count(), 0, "base 11 is unbound on ns_pro");
    send(&h, GamepadEvent::ButtonUp(11));
    engine.shutdown();
}

#[test]
fn ipc_rebind_while_page_key_held_releases_it() {
    // Review focus 1 + 2 through the engine: rebinding (config_write bumps
    // the generation) and releasing the trigger first must not strand F1.
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config.example.json");
    let mut cfg = Config::load_from_path(&path).unwrap();
    cfg.pad_mut(ControllerKind::DualSense)
        .table_mut(Page::QuickL)
        .insert("11".into(), ButtonEntry { label: String::new(), binding: Binding::Key("F1".into()) });
    let engine = Engine::spawn_with_fake_gamepad(cfg).unwrap();
    let h = engine.handle();

    send(&h, GamepadEvent::Connected(ControllerKind::DualSense));
    send(&h, GamepadEvent::Trigger { axis: 4, value: 1.0 });
    send(&h, GamepadEvent::ButtonDown(11));
    assert_eq!(h.held_keys_count(), 1);
    {
        let mut live = h.config_write();
        live.pad_mut(ControllerKind::DualSense).quick_l.clear();
        live.pad_mut(ControllerKind::DualSense).buttons.get_mut("11").unwrap().binding =
            Binding::Key("W".into());
    }
    std::thread::sleep(STEP);
    send(&h, GamepadEvent::Trigger { axis: 4, value: 0.0 });
    send(&h, GamepadEvent::ButtonUp(11));
    assert_eq!(h.held_keys_count(), 0, "F1 released even though it is no longer bound anywhere");
    engine.shutdown();
}
