//! Filesystem watcher for `padtokbm.json`. When the user edits the
//! config in an external editor (Notepad, VSCode, etc.), notify-rs fires;
//! we debounce 250ms (some editors save by writing the file 3+ times in
//! a row) and emit a single `()` to the consumer, who reloads via
//! `ConfigDoc::load`.
//!
//! The debounce is trailing-edge: the signal goes out once the file has
//! been quiet for 250ms, so the reload always reads the last save. (Up to
//! v2.6.0 it was leading-edge and dropped every change in the 250ms after
//! a signal — a second save within that window was never reloaded.)

use anyhow::Result;
use crossbeam_channel::{unbounded, RecvTimeoutError, Sender};
use notify::{Config as NConfig, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::time::Duration;

const DEBOUNCE: Duration = Duration::from_millis(250);

pub fn spawn(path: PathBuf, tx: Sender<()>) -> Result<RecommendedWatcher> {
    let (raw_tx, raw_rx) = unbounded::<()>();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        if let Ok(ev) = res {
            if matches!(ev.kind, notify::EventKind::Modify(_) | notify::EventKind::Create(_)) {
                let _ = raw_tx.send(());
            }
        }
    })?;
    // Ends when the watcher (and with it `raw_tx`) is dropped, or when the
    // consumer hangs up.
    std::thread::spawn(move || {
        while raw_rx.recv().is_ok() {
            loop {
                match raw_rx.recv_timeout(DEBOUNCE) {
                    Ok(()) => continue,
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            if tx.send(()).is_err() {
                return;
            }
        }
    });
    watcher.configure(NConfig::default().with_poll_interval(Duration::from_millis(500)))?;
    watcher.watch(&path, RecursiveMode::NonRecursive)?;
    Ok(watcher)
}
