use dualsense_mapper::config_io::{write_atomic, ConfigDoc};
use serde_json::json;

// Full 25-button FIXTURE (buttons 0..=24).  Buttons not relevant to the
// test intent are set to "unbound"; the macro reference on button 23 is
// satisfied by the "macro_A" entry in "macros".
const FIXTURE: &str = r#"{
  "_help": ["this is doc"],
  "_keyboard_keys": { "letters": ["a", "b"] },
  "version": 1,
  "deadzone": 0.4,
  "trigger_threshold": 0.5,
  "min_press_ms": [8, 25],
  "tick_jitter_ms": [0, 3],
  "log_events": true,
  "buttons": {
    "0":  { "label": "Cross",      "type": "key",     "value": "x" },
    "1":  { "label": "Circle",     "type": "unbound" },
    "2":  { "label": "Square",     "type": "unbound" },
    "3":  { "label": "Triangle",   "type": "unbound" },
    "4":  { "label": "Share",      "type": "unbound" },
    "5":  { "label": "PS",         "type": "unbound" },
    "6":  { "label": "Options",    "type": "unbound" },
    "7":  { "label": "L3",         "type": "unbound" },
    "8":  { "label": "R3",         "type": "unbound" },
    "9":  { "label": "L1",         "type": "unbound" },
    "10": { "label": "R1",         "type": "unbound" },
    "11": { "label": "DPad Up",    "type": "unbound" },
    "12": { "label": "DPad Down",  "type": "unbound" },
    "13": { "label": "DPad Left",  "type": "unbound" },
    "14": { "label": "DPad Right", "type": "unbound" },
    "15": { "label": "LS Up",      "type": "unbound" },
    "16": { "label": "LS Down",    "type": "unbound" },
    "17": { "label": "LS Left",    "type": "unbound" },
    "18": { "label": "LS Right",   "type": "unbound" },
    "19": { "label": "RS Up",      "type": "unbound" },
    "20": { "label": "RS Down",    "type": "unbound" },
    "21": { "label": "RS Left",    "type": "unbound" },
    "22": { "label": "RS Right",   "type": "unbound" },
    "23": { "label": "L2",         "type": "macro",   "value": "macro_A" },
    "24": { "label": "R2",         "type": "unbound" }
  },
  "macros": {
    "macro_A": {
      "loop": false,
      "steps": [
        { "key": "Left", "action": "down", "delay_ms": [50, 80] },
        { "key": "Left", "action": "up",   "delay_ms": [15, 25] }
      ]
    }
  }
}"#;

#[test]
fn round_trip_preserves_help_fields() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), FIXTURE).unwrap();

    let doc = ConfigDoc::load(tmp.path()).expect("load");
    write_atomic(tmp.path(), &doc).expect("write");

    let on_disk: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tmp.path()).unwrap()).unwrap();
    assert_eq!(on_disk["_help"], json!(["this is doc"]));
    assert_eq!(on_disk["_keyboard_keys"]["letters"], json!(["a", "b"]));
}

#[test]
fn updating_deadzone_preserves_help_and_changes_value() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), FIXTURE).unwrap();

    let mut doc = ConfigDoc::load(tmp.path()).unwrap();
    doc.set_deadzone(0.6);
    write_atomic(tmp.path(), &doc).unwrap();

    let on_disk: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tmp.path()).unwrap()).unwrap();
    assert_eq!(on_disk["_help"], json!(["this is doc"]));
    // 0.6 cannot be represented exactly as f32; allow epsilon tolerance.
    let dz = on_disk["deadzone"].as_f64().unwrap();
    assert!((dz - 0.6_f64).abs() < 1e-4, "deadzone should be ~0.6, got {dz}");
}

#[test]
fn invalid_payload_fails_validation_and_does_not_touch_disk() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), FIXTURE).unwrap();

    let mut doc = ConfigDoc::load(tmp.path()).unwrap();
    doc.set_min_press_ms_unchecked([25, 8]); // min >= max — invalid
    let err = doc.validate().expect_err("should reject");
    assert!(format!("{err}").contains("min_press_ms"));

    let on_disk = std::fs::read_to_string(tmp.path()).unwrap();
    assert_eq!(on_disk.trim(), FIXTURE.trim(), "disk untouched on validation failure");
}

#[test]
fn set_mapping_enabled_preserves_help() {
    use dualsense_mapper::gamepad::ControllerKind;
    let example = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config.example.json");
    let orig: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&example).unwrap()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.json");
    std::fs::copy(&example, &path).unwrap();
    let mut doc = ConfigDoc::load(&path).unwrap();
    doc.set_mapping_enabled(ControllerKind::DualSense, false);
    write_atomic(&path, &doc).unwrap();
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(raw["mapping_enabled"]["dualsense"], false);
    assert_eq!(raw["_help"], orig["_help"]);
}
