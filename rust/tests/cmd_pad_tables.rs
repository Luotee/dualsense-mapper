//! Binding IPC per pad and page: set, swap, trigger mode, clear, reset,
//! whole-pad write. Each writes disk and the live engine, or neither.

use dualsense_mapper::config::{Binding, ButtonEntry, Config};
use dualsense_mapper::engine::{Engine, Handle};
use dualsense_mapper::gamepad::ControllerKind;
use dualsense_mapper::gui::commands::{
    clear_pad_impl, reset_pad_impl, set_binding_impl, set_pad_tables_impl,
    set_trigger_mode_impl, swap_bindings_impl,
};
use dualsense_mapper::pad_bindings::{default_pad_bindings, Page, Side, TriggerMode};
use tempfile::TempDir;

use ControllerKind::{DualSense, Ns2Pro, NsPro};

struct Fixture {
    _dir: TempDir,
    path: std::path::PathBuf,
    engine: Engine,
}

impl Fixture {
    fn h(&self) -> Handle {
        self.engine.handle()
    }
    fn disk(&self) -> Config {
        Config::load_from_path(&self.path).unwrap()
    }
    fn raw(&self) -> String {
        std::fs::read_to_string(&self.path).unwrap()
    }
}

/// The shipped example (default tables) on disk, and an engine with it.
fn fixture() -> Fixture {
    let example = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config.example.json");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("padtokbm.json");
    std::fs::copy(&example, &path).unwrap();
    let engine = Engine::spawn_with_fake_gamepad(Config::load_from_path(&path).unwrap()).unwrap();
    Fixture { _dir: dir, path, engine }
}

fn key(k: &str) -> ButtonEntry {
    ButtonEntry { label: "slot".into(), binding: Binding::Key(k.into()) }
}

fn binding(cfg: &Config, pad: ControllerKind, page: Page, id: u32) -> Option<Binding> {
    cfg.pad(pad).table(page).get(&id.to_string()).map(|e| e.binding.clone())
}

#[test]
fn set_binding_on_one_pad_leaves_others() {
    let f = fixture();
    set_binding_impl(&f.h(), &f.path, Ns2Pro, Page::QuickR, 0, key("F5")).unwrap();
    for cfg in [f.disk(), f.h().config_read().clone()] {
        assert_eq!(binding(&cfg, Ns2Pro, Page::QuickR, 0), Some(Binding::Key("F5".into())));
        assert!(cfg.pad(DualSense).quick_r.is_empty());
        assert!(cfg.pad(NsPro).quick_r.is_empty());
    }
    f.engine.shutdown();
}

#[test]
fn set_binding_rejects_quick_page_id_5() {
    let f = fixture();
    let before = f.raw();
    let err = set_binding_impl(&f.h(), &f.path, DualSense, Page::QuickL, 5, key("F1")).unwrap_err();
    assert!(format!("{err:#}").contains('5'), "got: {err:#}");
    assert_eq!(f.raw(), before, "disk unchanged");
    assert!(f.h().config_read().pad(DualSense).quick_l.is_empty(), "engine unchanged");
    f.engine.shutdown();
}

#[test]
fn swap_bindings_across_base_and_page_is_one_write() {
    let f = fixture();
    set_binding_impl(&f.h(), &f.path, DualSense, Page::QuickL, 11, key("F1")).unwrap();
    swap_bindings_impl(&f.h(), &f.path, DualSense, (Page::Base, 11), (Page::QuickL, 11)).unwrap();
    for cfg in [f.disk(), f.h().config_read().clone()] {
        assert_eq!(binding(&cfg, DualSense, Page::Base, 11), Some(Binding::Key("F1".into())));
        assert_eq!(binding(&cfg, DualSense, Page::QuickL, 11), Some(Binding::Key("Up".into())));
        assert_eq!(cfg.pad(DualSense).buttons["11"].label, "D-pad ↑", "slot labels stay");
    }
    f.engine.shutdown();
}

#[test]
fn swap_with_an_empty_page_slot_moves_the_binding() {
    let f = fixture();
    swap_bindings_impl(&f.h(), &f.path, NsPro, (Page::Base, 12), (Page::QuickR, 3)).unwrap();
    let cfg = f.disk();
    assert_eq!(binding(&cfg, NsPro, Page::Base, 12), Some(Binding::Unbound));
    assert_eq!(binding(&cfg, NsPro, Page::QuickR, 3), Some(Binding::Key("Down".into())));
    f.engine.shutdown();
}

#[test]
fn swap_rejects_a_bad_page_id_and_writes_nothing() {
    let f = fixture();
    let before = f.raw();
    swap_bindings_impl(&f.h(), &f.path, DualSense, (Page::Base, 11), (Page::QuickL, 9)).unwrap_err();
    assert_eq!(f.raw(), before);
    assert_eq!(binding(&f.h().config_read(), DualSense, Page::Base, 11), Some(Binding::Key("Up".into())));
    f.engine.shutdown();
}

#[test]
fn clear_pad_keeps_trigger_mode_and_labels() {
    let f = fixture();
    set_trigger_mode_impl(&f.h(), &f.path, DualSense, Side::L, TriggerMode::Key).unwrap();
    set_binding_impl(&f.h(), &f.path, DualSense, Page::QuickR, 1, key("F2")).unwrap();
    clear_pad_impl(&f.h(), &f.path, DualSense).unwrap();
    for cfg in [f.disk(), f.h().config_read().clone()] {
        let p = cfg.pad(DualSense);
        assert!(p.buttons.values().all(|e| e.binding == Binding::Unbound));
        assert!(p.quick_l.is_empty() && p.quick_r.is_empty());
        assert_eq!(p.trigger_mode.l, TriggerMode::Key);
        assert_eq!(p.buttons["0"].label, "Cross（叉叉）");
        assert_eq!(cfg.pad(NsPro), &default_pad_bindings(NsPro), "other pads untouched");
    }
    f.engine.shutdown();
}

#[test]
fn reset_pad_restores_spec_defaults() {
    let f = fixture();
    set_binding_impl(&f.h(), &f.path, Ns2Pro, Page::Base, 0, key("z")).unwrap();
    set_trigger_mode_impl(&f.h(), &f.path, Ns2Pro, Side::R, TriggerMode::Key).unwrap();
    reset_pad_impl(&f.h(), &f.path, Ns2Pro).unwrap();
    assert_eq!(f.disk().pad(Ns2Pro), &default_pad_bindings(Ns2Pro));
    assert_eq!(f.h().config_read().pad(Ns2Pro), &default_pad_bindings(Ns2Pro));
    f.engine.shutdown();
}

#[test]
fn set_pad_tables_round_trips_and_validates() {
    let f = fixture();
    let mut tables = default_pad_bindings(NsPro).cleared();
    tables.quick_l.insert("13".into(), key("F3"));
    tables.trigger_mode.r = TriggerMode::Key;
    set_pad_tables_impl(&f.h(), &f.path, NsPro, tables.clone()).unwrap();
    assert_eq!(f.disk().pad(NsPro), &tables);
    assert_eq!(f.h().config_read().pad(NsPro), &tables);

    let before = f.raw();
    let mut bad = tables.clone();
    bad.quick_l.insert("20".into(), key("F4"));
    let err = set_pad_tables_impl(&f.h(), &f.path, NsPro, bad).unwrap_err();
    assert!(format!("{err:#}").contains("quick_l"), "got: {err:#}");
    assert_eq!(f.raw(), before, "disk unchanged");
    assert_eq!(f.h().config_read().pad(NsPro), &tables, "engine unchanged");
    f.engine.shutdown();
}

#[test]
fn set_trigger_mode_persists_and_reaches_engine() {
    let f = fixture();
    set_trigger_mode_impl(&f.h(), &f.path, NsPro, Side::L, TriggerMode::Key).unwrap();
    assert_eq!(f.disk().pad(NsPro).trigger_mode.l, TriggerMode::Key);
    assert_eq!(f.h().config_read().pad(NsPro).trigger_mode.l, TriggerMode::Key);
    assert_eq!(f.h().config_read().pad(NsPro).trigger_mode.r, TriggerMode::Page);
    f.engine.shutdown();
}

#[test]
fn writes_keep_help_fields() {
    let f = fixture();
    let help = |s: &str| serde_json::from_str::<serde_json::Value>(s).unwrap()["_help"].clone();
    let before = help(&f.raw());
    reset_pad_impl(&f.h(), &f.path, DualSense).unwrap();
    assert_eq!(help(&f.raw()), before);
    f.engine.shutdown();
}

#[test]
fn config_folder_is_the_parent_directory() {
    use dualsense_mapper::gui::commands::config_folder;
    let p = std::path::Path::new("/tmp/x/padtokbm.json");
    assert_eq!(config_folder(p), std::path::PathBuf::from("/tmp/x"));
    assert_eq!(config_folder(std::path::Path::new("padtokbm.json")), std::path::PathBuf::from("."));
}
