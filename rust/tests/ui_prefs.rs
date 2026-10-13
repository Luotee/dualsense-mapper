use dualsense_mapper::gui::ui_prefs::{load, save, UiPrefs};

#[test]
fn last_pad_round_trips_and_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("dualsense-mapper.ui.json");
    save(&p, &UiPrefs { last_pad: Some("ns2_pro".into()), ..Default::default() }).unwrap();
    assert_eq!(load(&p).last_pad.as_deref(), Some("ns2_pro"));
    std::fs::write(&p, r#"{"drawer_open": true}"#).unwrap();
    let old = load(&p);
    assert_eq!(old.last_pad, None);
    assert!(old.drawer_open);
    std::fs::write(&p, "not json").unwrap();
    assert_eq!(load(&p).last_pad, None);
}
