//! The board's drag-and-drop (tile → slot, slot ↔ slot, drag-out clear,
//! macro step order) is HTML5 DnD. On Windows, Tauri's own drag-drop handler
//! (`dragDropEnabled`, default true) swallows it in WebView2, and web-smoke
//! cannot see that (it runs in plain Chromium). Pin the setting here.

#[test]
fn main_window_leaves_drag_and_drop_to_the_page() {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
    let conf: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    let windows = conf["app"]["windows"].as_array().expect("app.windows");
    assert!(!windows.is_empty());
    for w in windows {
        assert_eq!(w["dragDropEnabled"], serde_json::Value::Bool(false), "window {}: dragDropEnabled must be false", w["label"]);
    }
}
