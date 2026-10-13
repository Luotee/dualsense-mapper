//! UI-only persistence: drawer-open state, last-selected tab, etc.
//!
//! Lives at `<exe-dir>/padtokbm.ui.json`, beside the main config (an old
//! `dualsense-mapper.ui.json` there is adopted on first use).
//! Out of band from `Config::validate` — corruption / missing file just
//! resets to defaults. Never blocks the engine or the GUI startup.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UiPrefs {
    #[serde(default)]
    pub drawer_open: bool,
    #[serde(default)]
    pub last_tab: Option<String>,
    /// The pad the board showed last (`ControllerKind::slug()`), restored on
    /// start when no controller is connected. `drawer_open` / `last_tab`
    /// are left over from the tabbed GUI and unused by the board.
    #[serde(default)]
    pub last_pad: Option<String>,
}

pub fn path_beside(config_path: &std::path::Path) -> PathBuf {
    // Sit next to the running config. Same directory; different filename.
    use crate::file_names::{adopt_legacy, log_adoption, LEGACY_UI_PREFS_FILE, UI_PREFS_FILE};
    let dir = config_path.parent().unwrap_or_else(|| std::path::Path::new(""));
    log_adoption(&adopt_legacy(dir, LEGACY_UI_PREFS_FILE, UI_PREFS_FILE));
    dir.join(UI_PREFS_FILE)
}

pub fn load(path: &std::path::Path) -> UiPrefs {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(path: &std::path::Path, prefs: &UiPrefs) -> Result<()> {
    let json = serde_json::to_string_pretty(prefs)?;
    std::fs::write(path, json)?;
    Ok(())
}
