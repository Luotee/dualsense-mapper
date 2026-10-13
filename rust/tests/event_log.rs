//! `log_events: true` writes `padtokbm-events.log` beside the config
//! (v3.1.2: the switch existed since v2 but no writer did).

#[allow(dead_code)]
mod test_helpers;

use dualsense_mapper::engine::Engine;
use dualsense_mapper::event_log::{format_line, EventLog, LOG_FILE, ROTATE_BYTES};
use dualsense_mapper::gamepad::{ControllerKind, GamepadEvent};
use std::fs;
use std::time::Duration;

#[test]
fn format_line_shape() {
    let line = format_line(1_760_344_919_352, "按下", "送出 Alt 按下");
    let cols: Vec<&str> = line.trim_end_matches('\n').split('\t').collect();
    assert!(line.ends_with('\n'));
    assert_eq!(cols.len(), 3, "{line:?}");
    assert_eq!(&cols[1..], ["按下", "送出 Alt 按下"]);
    // "YYYY-MM-DD HH:MM:SS.mmm" local time; milliseconds survive.
    let ts = cols[0];
    assert_eq!(ts.len(), 23, "{ts}");
    assert_eq!(&ts[4..5], "-");
    assert_eq!(&ts[10..11], " ");
    assert_eq!(&ts[19..], ".352");
}

#[test]
fn disabled_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = EventLog::new(Some(dir.path().to_path_buf()));
    log.append(false, "x\n");
    assert!(!dir.path().join(LOG_FILE).exists());
    let mut none = EventLog::new(None);
    none.append(true, "x\n");
}

#[test]
fn enabled_appends_lines() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = EventLog::new(Some(dir.path().to_path_buf()));
    log.append(true, "a\n");
    log.append(true, "b\n");
    assert_eq!(fs::read_to_string(dir.path().join(LOG_FILE)).unwrap(), "a\nb\n");
}

#[test]
fn rotates_at_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(LOG_FILE);
    fs::write(&path, vec![b'x'; (ROTATE_BYTES - 5) as usize]).unwrap();
    fs::write(dir.path().join(format!("{LOG_FILE}.1")), "old").unwrap();
    let mut log = EventLog::new(Some(dir.path().to_path_buf()));
    log.append(true, "0123456789\n");
    let rotated = fs::metadata(dir.path().join(format!("{LOG_FILE}.1"))).unwrap().len();
    assert_eq!(rotated, ROTATE_BYTES - 5, "old .1 replaced by the full file");
    assert_eq!(fs::read_to_string(&path).unwrap(), "0123456789\n");
}

#[cfg(unix)]
#[test]
fn unwritable_log_dir_is_harmless() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o555)).unwrap();
    let mut log = EventLog::new(Some(dir.path().to_path_buf()));
    log.append(true, "a\n");
    log.append(true, "b\n");
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!dir.path().join(LOG_FILE).exists());
}

#[test]
fn engine_writes_key_events_when_enabled() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = test_helpers::load_example();
    cfg.log_events = true;
    cfg.mapping_enabled.insert("dualsense".into(), true);
    let engine = Engine::spawn_with_fake_gamepad(cfg).expect("spawn");
    let h = engine.handle();
    h.set_log_dir(dir.path().to_path_buf());
    h.fake_event(GamepadEvent::Connected(ControllerKind::DualSense));
    std::thread::sleep(Duration::from_millis(100));
    h.fake_button_down(0); // bound to a key in maple_artale.json
    std::thread::sleep(Duration::from_millis(150));
    h.fake_button_up(0);
    std::thread::sleep(Duration::from_millis(150));
    engine.shutdown();
    let text = fs::read_to_string(dir.path().join(LOG_FILE)).expect("log file written");
    assert!(text.lines().any(|l| l.split('\t').nth(1) == Some("按下")), "{text}");
    assert!(text.lines().any(|l| l.split('\t').nth(1) == Some("放開")), "{text}");
}

#[test]
fn engine_writes_nothing_when_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = test_helpers::load_example();
    cfg.log_events = false;
    cfg.mapping_enabled.insert("dualsense".into(), true);
    let engine = Engine::spawn_with_fake_gamepad(cfg).expect("spawn");
    let h = engine.handle();
    h.set_log_dir(dir.path().to_path_buf());
    h.fake_event(GamepadEvent::Connected(ControllerKind::DualSense));
    std::thread::sleep(Duration::from_millis(100));
    h.fake_button_down(0);
    std::thread::sleep(Duration::from_millis(150));
    engine.shutdown();
    assert!(!dir.path().join(LOG_FILE).exists());
}
