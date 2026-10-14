//! The `log_events` file: every synthesized key and every macro start, one
//! tab-separated line each, in `padtokbm-events.log` beside the config, so a
//! user can audit what the program sent (README FAQ「會不會被當成外掛？」).
//!
//! Written by the engine thread only while `log_events` is true. At
//! [`ROTATE_BYTES`] the file is renamed to `padtokbm-events.log.1` (an older
//! `.1` is replaced) and a new one is started. An I/O error never stops the
//! engine: it is logged once per session and the line is dropped.

use std::io::Write;
use std::path::PathBuf;

pub const LOG_FILE: &str = "padtokbm-events.log";
pub const ROTATE_BYTES: u64 = 1 << 20;

/// `"2026-10-13 16:41:59.352\t按下\t送出 Alt 按下\tsrc=btn:0\n"` — local
/// time; `src` is what sent it (`btn:<id>` / `macro:<name>`, v3.2.0).
pub fn format_line(ts_ms: u64, kind: &str, text: &str, src: &str) -> String {
    use chrono::TimeZone;
    let ts = chrono::Local
        .timestamp_millis_opt(ts_ms as i64)
        .single()
        .map(|t| t.format("%Y-%m-%d %H:%M:%S%.3f").to_string())
        .unwrap_or_else(|| format!("{ts_ms}"));
    format!("{ts}\t{kind}\t{text}\tsrc={src}\n")
}

pub struct EventLog {
    dir: Option<PathBuf>,
    warned: bool,
}

impl EventLog {
    /// `dir`: the config's folder (`None`: nowhere to write, every append is
    /// a no-op).
    pub fn new(dir: Option<PathBuf>) -> Self {
        EventLog { dir, warned: false }
    }

    pub fn set_dir(&mut self, dir: Option<PathBuf>) {
        self.dir = dir;
        self.warned = false;
    }

    /// Append `line` (already newline-terminated) when `enabled`.
    pub fn append(&mut self, enabled: bool, line: &str) {
        if !enabled {
            return;
        }
        let Some(dir) = self.dir.as_ref() else { return };
        let path = dir.join(LOG_FILE);
        let result = (|| -> std::io::Result<()> {
            let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if len > 0 && len + line.len() as u64 > ROTATE_BYTES {
                std::fs::rename(&path, dir.join(format!("{LOG_FILE}.1")))?;
            }
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
            f.write_all(line.as_bytes())
        })();
        if let Err(e) = result {
            if !self.warned {
                self.warned = true;
                tracing::warn!("event log {}: {e}; not writing it this session", path.display());
            }
        }
    }
}
