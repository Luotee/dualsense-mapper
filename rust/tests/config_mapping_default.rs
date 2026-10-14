//! v3.2.0: every pad defaults to keyboard/mouse output (鍵鼠). Existing
//! configs keep what they say: an explicit `"ns2_pro": false` stays Xbox.

#[allow(dead_code)]
mod test_helpers;

use dualsense_mapper::config::Config;
use dualsense_mapper::gamepad::ControllerKind;

#[test]
fn missing_key_means_mapping_on_for_every_pad() {
    let mut cfg = test_helpers::load_example();
    cfg.mapping_enabled.clear();
    for k in ControllerKind::ALL {
        assert!(cfg.mapping_enabled_for(k), "{k:?}");
    }
}

#[test]
fn explicit_false_still_wins() {
    let mut cfg = test_helpers::load_example();
    cfg.mapping_enabled.clear();
    cfg.mapping_enabled.insert("ns2_pro".into(), false);
    assert!(!cfg.mapping_enabled_for(ControllerKind::Ns2Pro));
    assert!(cfg.mapping_enabled_for(ControllerKind::DualSense));
}

#[test]
fn example_config_maps_every_pad() {
    let text = include_str!("../config.example.json");
    let cfg = Config::load_from_str(text).expect("example parses");
    for k in ControllerKind::ALL {
        assert_eq!(cfg.mapping_enabled.get(k.slug()), Some(&true), "{k:?}");
    }
}
