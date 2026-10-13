use dualsense_mapper::config::Config;
use dualsense_mapper::gamepad::ControllerKind;
use dualsense_mapper::pad_bindings::default_pad_bindings;
use std::path::PathBuf;

#[test]
fn shipped_example_config_validates() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config.example.json");
    let cfg = Config::load_from_path(&path).expect("load+validate");
    assert_eq!(cfg.version, 2);
    for k in ControllerKind::ALL {
        assert_eq!(cfg.pad(k).buttons.len(), 33, "{k:?}");
    }
}

#[test]
fn example_file_is_version_2_with_default_pads() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config.example.json");
    let cfg = Config::load_from_path(&path).expect("load+validate");
    assert_eq!(cfg.version, 2);
    assert!(cfg.macros.is_empty(), "spec 4.8: no macros by default");
    // Read the file's own pads, not the filled-in ones: the example must
    // carry all three tables itself.
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    for k in ControllerKind::ALL {
        assert!(raw["pads"][k.slug()].is_object(), "example lists pad {}", k.slug());
        let want = default_pad_bindings(k);
        let got = cfg.pad(k);
        assert_eq!(got.trigger_mode, want.trigger_mode, "{k:?}");
        assert_eq!(got.quick_l, want.quick_l, "{k:?}");
        assert_eq!(got.quick_r, want.quick_r, "{k:?}");
        for (id, e) in &want.buttons {
            assert_eq!(got.buttons[id].binding, e.binding, "{k:?} id {id}");
        }
    }
}

#[test]
fn shipped_example_config_has_rumble_on_connect() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config.example.json");
    let text = std::fs::read_to_string(&path).expect("read example config");
    let cfg = Config::load_from_path(&path).expect("load+validate");
    assert!(cfg.rumble_on_connect);
    assert!(text.contains("\"rumble_on_connect\""), "example config must carry the key");
}

#[test]
fn shipped_maple_profile_validates() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/maple_artale.json");
    Config::load_from_path(&path).expect("maple profile load+validate");
}
