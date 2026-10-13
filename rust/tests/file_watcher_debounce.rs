//! The config file watcher must always end a burst of saves with a reload
//! signal that comes AFTER the last save, so the consumer reads the final
//! file content. v2.6.0 hardware run: mapping ON then OFF saved 63 ms apart
//! → the old leading-edge debounce signalled on ON and dropped OFF, leaving
//! the app in mapping mode until the next save.

use crossbeam_channel::unbounded;
use dualsense_mapper::gui::file_watcher;
use std::time::{Duration, Instant};

/// Collects every signal for `window`, returning each arrival time.
/// Callers that write while collecting use [`collect_in_background`] so a
/// signal is stamped when it is sent, not when the test gets round to it.
fn collect(rx: &crossbeam_channel::Receiver<()>, window: Duration) -> Vec<Instant> {
    let end = Instant::now() + window;
    let mut got = Vec::new();
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(()) => got.push(Instant::now()),
            Err(_) => break,
        }
    }
    got
}

fn collect_in_background(
    rx: crossbeam_channel::Receiver<()>,
    window: Duration,
) -> std::thread::JoinHandle<Vec<Instant>> {
    std::thread::spawn(move || collect(&rx, window))
}

fn watched_file() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("padtokbm.json");
    std::fs::write(&path, "{\"v\":0}").unwrap();
    (dir, path)
}

#[test]
fn second_save_within_debounce_window_is_signalled_after_it() {
    let (_dir, path) = watched_file();
    let (tx, rx) = unbounded();
    let _w = file_watcher::spawn(path.clone(), tx).unwrap();
    collect(&rx, Duration::from_millis(400)); // settle
    let collector = collect_in_background(rx, Duration::from_millis(1500));

    std::fs::write(&path, "{\"v\":1}").unwrap();
    std::thread::sleep(Duration::from_millis(63));
    std::fs::write(&path, "{\"v\":2}").unwrap();
    let last_save = Instant::now();

    let got = collector.join().unwrap();
    assert!(
        got.iter().any(|t| *t >= last_save),
        "no reload signal after the last save ({} signal(s), all before it)",
        got.len()
    );
}

#[test]
fn burst_of_saves_coalesces_into_one_signal() {
    let (_dir, path) = watched_file();
    let (tx, rx) = unbounded();
    let _w = file_watcher::spawn(path.clone(), tx).unwrap();
    collect(&rx, Duration::from_millis(400)); // settle
    let collector = collect_in_background(rx, Duration::from_millis(1500));

    for v in 1..=3 {
        std::fs::write(&path, format!("{{\"v\":{v}}}")).unwrap();
        std::thread::sleep(Duration::from_millis(20));
    }

    let got = collector.join().unwrap();
    assert_eq!(got.len(), 1, "a 3-write save burst must reload exactly once");
}
