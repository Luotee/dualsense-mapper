use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub version: u32,
    pub deadzone: f32,
    pub trigger_threshold: f32,
    pub min_press_ms: [u32; 2],
    pub tick_jitter_ms: [u32; 2],
    pub log_events: bool,
    pub buttons: BTreeMap<String, ButtonEntry>,
    pub macros: BTreeMap<String, MacroDef>,
    #[serde(default = "default_touchpad_cursor_enabled")]
    pub touchpad_cursor_enabled: bool,
    #[serde(default = "default_touchpad_cursor_sensitivity")]
    pub touchpad_cursor_sensitivity: f32,
    #[serde(default = "default_touchpad_midpoint_x")]
    pub touchpad_midpoint_x: u16,
    #[serde(default = "default_touchpad_midpoint_y")]
    pub touchpad_midpoint_y: u16,
    #[serde(default = "default_touchpad_accel_slow_threshold")]
    pub touchpad_accel_slow_threshold: u32,
    #[serde(default = "default_touchpad_accel_fast_threshold")]
    pub touchpad_accel_fast_threshold: u32,
    #[serde(default = "default_touchpad_accel_gain_slow")]
    pub touchpad_accel_gain_slow: f32,
    #[serde(default = "default_touchpad_accel_gain_fast")]
    pub touchpad_accel_gain_fast: f32,
    #[serde(default = "default_touchpad_deadzone_radius")]
    pub touchpad_deadzone_radius: u32,
    #[serde(default = "default_touchpad_click_freeze_enabled")]
    pub touchpad_click_freeze_enabled: bool,
    /// Per-controller mapping switch, keyed by `ControllerKind::slug()`.
    /// Missing key → `ControllerKind::default_mapping_enabled()`.
    #[serde(default)]
    pub mapping_enabled: BTreeMap<String, bool>,
}

fn default_touchpad_cursor_enabled() -> bool { true }
fn default_touchpad_cursor_sensitivity() -> f32 { 1.5 }
fn default_touchpad_midpoint_x() -> u16 { 960 }
fn default_touchpad_midpoint_y() -> u16 { 540 }
fn default_touchpad_accel_slow_threshold() -> u32 { 5 }
fn default_touchpad_accel_fast_threshold() -> u32 { 20 }
fn default_touchpad_accel_gain_slow() -> f32 { 0.5 }
fn default_touchpad_accel_gain_fast() -> f32 { 1.5 }
fn default_touchpad_deadzone_radius() -> u32 { 2 }
fn default_touchpad_click_freeze_enabled() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ButtonEntry {
    pub label: String,
    #[serde(flatten)]
    pub binding: Binding,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "value", rename_all = "lowercase")]
pub enum Binding {
    Key(String),
    Macro(String),
    Mouse(MouseButton),
    Unbound,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MacroDef {
    #[serde(rename = "loop")]
    pub repeat: bool,
    pub steps: Vec<MacroStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MacroStep {
    pub key: String,
    pub action: StepAction,
    pub delay_ms: [u32; 2],
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum StepAction {
    Down,
    Up,
}

use anyhow::{anyhow, bail, Context, Result};
use std::path::Path;

pub const VALID_BUTTON_IDS: std::ops::RangeInclusive<u32> = 0..=28;

/// Button ids added in v2.1.0 for the four touchpad quadrants. Missing
/// entries in a loaded config are auto-filled as `Unbound` before
/// validation so v2.0 configs continue to load.
pub const TOUCHPAD_QUADRANT_IDS: [u32; 4] = [25, 26, 27, 28];
const TOUCHPAD_QUADRANT_LABELS: [&str; 4] = [
    "Touchpad TL",
    "Touchpad TR",
    "Touchpad BL",
    "Touchpad BR",
];

impl Config {
    pub fn load_from_path(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config: {}", path.display()))?;
        let mut cfg: Config = serde_json::from_str(&text)
            .with_context(|| format!("parsing config: {}", path.display()))?;
        cfg.fill_touchpad_defaults();
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn load_from_str(s: &str) -> Result<Self> {
        let mut cfg: Config = serde_json::from_str(s)
            .context("parsing config from string")?;
        cfg.fill_touchpad_defaults();
        cfg.validate()?;
        Ok(cfg)
    }

    /// Insert default `Unbound` entries for any of the touchpad-quadrant
    /// ids (25..=28) missing from `buttons`. Called on load so that v2.0
    /// configs (which never had these ids) parse and validate cleanly.
    pub fn fill_touchpad_defaults(&mut self) {
        for (id, label) in TOUCHPAD_QUADRANT_IDS.iter().zip(TOUCHPAD_QUADRANT_LABELS.iter()) {
            let key = id.to_string();
            self.buttons.entry(key).or_insert_with(|| ButtonEntry {
                label: (*label).to_string(),
                binding: Binding::Unbound,
            });
        }
    }

    pub fn mapping_enabled_for(&self, kind: crate::gamepad::ControllerKind) -> bool {
        self.mapping_enabled
            .get(kind.slug())
            .copied()
            .unwrap_or_else(|| kind.default_mapping_enabled())
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            bail!("unsupported config version {}; expected 1", self.version);
        }
        if self.min_press_ms[0] >= self.min_press_ms[1] {
            bail!("min_press_ms range must have min < max, got {:?}", self.min_press_ms);
        }
        if self.tick_jitter_ms[0] > self.tick_jitter_ms[1] {
            bail!("tick_jitter_ms range must have min <= max, got {:?}", self.tick_jitter_ms);
        }
        if !(0.1..=10.0).contains(&self.touchpad_cursor_sensitivity) {
            bail!(
                "touchpad_cursor_sensitivity must be in [0.1, 10.0], got {}",
                self.touchpad_cursor_sensitivity
            );
        }
        if !(1..=4094).contains(&self.touchpad_midpoint_x) {
            bail!(
                "touchpad_midpoint_x must be in [1, 4094], got {}",
                self.touchpad_midpoint_x
            );
        }
        if !(1..=4094).contains(&self.touchpad_midpoint_y) {
            bail!(
                "touchpad_midpoint_y must be in [1, 4094], got {}",
                self.touchpad_midpoint_y
            );
        }
        if self.touchpad_accel_slow_threshold >= self.touchpad_accel_fast_threshold {
            bail!(
                "touchpad_accel_slow_threshold ({}) must be < touchpad_accel_fast_threshold ({})",
                self.touchpad_accel_slow_threshold,
                self.touchpad_accel_fast_threshold
            );
        }
        if !(0.1..=1.0).contains(&self.touchpad_accel_gain_slow) {
            bail!(
                "touchpad_accel_gain_slow must be in [0.1, 1.0], got {}",
                self.touchpad_accel_gain_slow
            );
        }
        if !(1.0..=5.0).contains(&self.touchpad_accel_gain_fast) {
            bail!(
                "touchpad_accel_gain_fast must be in [1.0, 5.0], got {}",
                self.touchpad_accel_gain_fast
            );
        }
        if self.touchpad_deadzone_radius > 50 {
            bail!(
                "touchpad_deadzone_radius must be <= 50, got {}",
                self.touchpad_deadzone_radius
            );
        }
        for slug in self.mapping_enabled.keys() {
            if crate::gamepad::ControllerKind::from_slug(slug).is_none() {
                bail!("mapping_enabled has unknown controller slug {slug:?}");
            }
        }
        for id in VALID_BUTTON_IDS {
            let key = id.to_string();
            if !self.buttons.contains_key(&key) {
                bail!("missing button id {id}; config must list all ids 0..=28");
            }
        }
        for k in self.buttons.keys() {
            let id: u32 = k.parse()
                .map_err(|_| anyhow!("unknown button id {k}; ids must be numeric 0..=28"))?;
            if !VALID_BUTTON_IDS.contains(&id) {
                bail!("unknown button id {id}; allowed range is 0..=28");
            }
        }
        for (id, entry) in &self.buttons {
            if let Binding::Macro(name) = &entry.binding {
                if !self.macros.contains_key(name) {
                    bail!("button {id} references unknown macro \"{name}\"");
                }
            }
        }
        for (id, entry) in &self.buttons {
            if let Binding::Key(k) = &entry.binding {
                parse_key(k).with_context(|| format!("button {id}"))?;
            }
        }
        for (name, m) in &self.macros {
            for (i, step) in m.steps.iter().enumerate() {
                parse_key(&step.key)
                    .with_context(|| format!("macro \"{name}\" step {i}"))?;
            }
        }
        for (name, m) in &self.macros {
            for (i, step) in m.steps.iter().enumerate() {
                if step.delay_ms[0] >= step.delay_ms[1] {
                    bail!(
                        "macro \"{name}\" step {i}: delay_ms range must have min < max, got {:?}",
                        step.delay_ms
                    );
                }
            }
        }
        Ok(())
    }
}

use enigo::Key;

pub fn parse_key(name: &str) -> Result<Key> {
    if name.chars().count() == 1 {
        let c = name.chars().next().unwrap();
        // On Windows, route ASCII chars through Key::Other(VK) so they
        // inject as real virtual-key events (auto-repeat fires, games
        // that read virtual-key state see them). Key::Unicode goes
        // through KEYEVENTF_UNICODE which inserts a character but does
        // NOT register as a held key.
        //
        // Linux keeps Key::Unicode as the simple fallback; the real
        // product only runs on Windows, this branch exists so cargo
        // test still works on a Linux dev host.
        #[cfg(target_os = "windows")]
        {
            if c.is_ascii_alphabetic() {
                // VK_A..VK_Z == 0x41..0x5A, same as ASCII upper-case codes.
                let vk = c.to_ascii_uppercase() as u32;
                return Ok(Key::Other(vk));
            }
            if c.is_ascii_digit() {
                // VK_0..VK_9 == 0x30..0x39, same as ASCII digit codes.
                return Ok(Key::Other(c as u32));
            }
            // OEM punctuation — VK codes assume the US keyboard layout.
            // Users on other layouts may need to bind by Unicode instead,
            // but every layout still produces the same scan-code that
            // these VKs map to in WM_KEYDOWN, so games hooked at the
            // virtual-key level will see the right key.
            if let Some(vk) = vk_for_oem_punct(c) {
                return Ok(Key::Other(vk));
            }
        }
        return Ok(Key::Unicode(c));
    }
    let lower = name.to_ascii_lowercase();
    // Left / right modifier specifics are Windows-only — on non-Windows
    // we fall back to the generic modifier so cargo test still passes.
    #[cfg(target_os = "windows")]
    {
        if let Some(vk) = vk_for_lr_modifier(&lower) {
            return Ok(Key::Other(vk));
        }
    }
    Ok(match lower.as_str() {
        "shift"   | "lshift" | "rshift" => Key::Shift,
        "control" | "ctrl" | "lcontrol" | "lctrl" | "rcontrol" | "rctrl" => Key::Control,
        "alt"     | "lalt" | "ralt" => Key::Alt,
        "meta" | "win" | "cmd" => Key::Meta,
        "left"    => Key::LeftArrow,
        "right"   => Key::RightArrow,
        "up"      => Key::UpArrow,
        "down"    => Key::DownArrow,
        "space"   => Key::Space,
        "enter" | "return" => Key::Return,
        "tab"     => Key::Tab,
        "escape" | "esc" => Key::Escape,
        "backspace" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "home"    => Key::Home,
        "end"     => Key::End,
        "pageup"  => Key::PageUp,
        "pagedown" => Key::PageDown,
        f if f.starts_with('f') && f[1..].parse::<u32>().is_ok() => {
            let n: u32 = f[1..].parse().unwrap();
            match n {
                1 => Key::F1, 2 => Key::F2, 3 => Key::F3, 4 => Key::F4,
                5 => Key::F5, 6 => Key::F6, 7 => Key::F7, 8 => Key::F8,
                9 => Key::F9, 10 => Key::F10, 11 => Key::F11, 12 => Key::F12,
                _ => bail!("unknown key name \"{name}\" (F-key out of range)"),
            }
        }
        _ => bail!("unknown key name \"{name}\""),
    })
}

/// Windows-only: VK codes for the OEM punctuation keys on a US layout.
/// Reference: <https://learn.microsoft.com/windows/win32/inputdev/virtual-key-codes>
#[cfg(target_os = "windows")]
fn vk_for_oem_punct(c: char) -> Option<u32> {
    Some(match c {
        '-' => 0xBD, // VK_OEM_MINUS
        '=' => 0xBB, // VK_OEM_PLUS
        ',' => 0xBC, // VK_OEM_COMMA
        '.' => 0xBE, // VK_OEM_PERIOD
        '/' => 0xBF, // VK_OEM_2  '/'
        '`' => 0xC0, // VK_OEM_3  '`'
        '[' => 0xDB, // VK_OEM_4  '['
        '\\' => 0xDC, // VK_OEM_5 '\'
        ']' => 0xDD, // VK_OEM_6  ']'
        '\'' => 0xDE, // VK_OEM_7 "'"
        ';' => 0xBA, // VK_OEM_1  ';'
        _ => return None,
    })
}

/// Windows-only: VK codes for the left/right-specific modifier keys.
#[cfg(target_os = "windows")]
fn vk_for_lr_modifier(lower: &str) -> Option<u32> {
    Some(match lower {
        "lshift" => 0xA0,   // VK_LSHIFT
        "rshift" => 0xA1,   // VK_RSHIFT
        "lcontrol" | "lctrl" => 0xA2, // VK_LCONTROL
        "rcontrol" | "rctrl" => 0xA3, // VK_RCONTROL
        "lalt" => 0xA4,     // VK_LMENU
        "ralt" => 0xA5,     // VK_RMENU
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_minimal_config() {
        let json = r#"{
            "version": 1,
            "deadzone": 0.4,
            "trigger_threshold": 0.5,
            "min_press_ms": [8, 25],
            "tick_jitter_ms": [0, 3],
            "log_events": true,
            "buttons": {
                "0": { "label": "Cross (X)", "type": "key", "value": "x" },
                "3": { "label": "Triangle", "type": "unbound" },
                "23": { "label": "L2", "type": "macro", "value": "macro_A" }
            },
            "macros": {
                "macro_A": {
                    "loop": true,
                    "steps": [
                        { "key": "Left", "action": "down", "delay_ms": [50, 80] }
                    ]
                }
            }
        }"#;

        let cfg: Config = serde_json::from_str(json).expect("parse");
        assert_eq!(cfg.version, 1);
        assert_eq!(cfg.buttons.len(), 3);
        match &cfg.buttons["0"].binding {
            Binding::Key(k) => assert_eq!(k, "x"),
            _ => panic!("expected key"),
        }
        match &cfg.buttons["3"].binding {
            Binding::Unbound => {}
            _ => panic!("expected unbound"),
        }
    }

    #[test]
    fn rejects_missing_button_id() {
        let mut cfg = sample_full_config();
        cfg.buttons.remove("7");
        let err = cfg.validate().unwrap_err().to_string();
        assert!(err.contains("missing button id 7"), "got: {err}");
    }

    #[test]
    fn rejects_extra_button_id() {
        let mut cfg = sample_full_config();
        cfg.buttons.insert("99".into(), ButtonEntry {
            label: "bogus".into(),
            binding: Binding::Unbound,
        });
        let err = cfg.validate().unwrap_err().to_string();
        assert!(err.contains("unknown button id 99"), "got: {err}");
    }

    #[test]
    fn rejects_unknown_macro_reference() {
        let mut cfg = sample_full_config();
        cfg.buttons.insert("23".into(), ButtonEntry {
            label: "L2".into(),
            binding: Binding::Macro("does_not_exist".into()),
        });
        let err = cfg.validate().unwrap_err().to_string();
        assert!(err.contains("references unknown macro \"does_not_exist\""), "got: {err}");
    }

    #[test]
    fn rejects_zero_range_delay() {
        let mut cfg = sample_full_config();
        let m = MacroDef {
            repeat: false,
            steps: vec![MacroStep {
                key: "Left".into(),
                action: StepAction::Down,
                delay_ms: [10, 10],
            }],
        };
        cfg.macros.insert("zero_range".into(), m);
        cfg.buttons.insert("23".into(), ButtonEntry {
            label: "L2".into(),
            binding: Binding::Macro("zero_range".into()),
        });
        let err = cfg.validate().unwrap_err().to_string();
        assert!(err.contains("delay_ms range must have min < max"), "got: {err}");
    }

    #[test]
    fn rejects_inverted_min_press_ms() {
        let mut cfg = sample_full_config();
        cfg.min_press_ms = [30, 8];
        let err = cfg.validate().unwrap_err().to_string();
        assert!(err.contains("min_press_ms"), "got: {err}");
    }

    #[test]
    fn parses_letter_keys() {
        assert!(matches!(parse_key("x"), Ok(_)));
        assert!(matches!(parse_key("A"), Ok(_)));
    }

    #[test]
    fn parses_named_keys() {
        assert!(matches!(parse_key("Shift"), Ok(_)));
        assert!(matches!(parse_key("Left"), Ok(_)));
        assert!(matches!(parse_key("Up"), Ok(_)));
        assert!(matches!(parse_key("F1"), Ok(_)));
    }

    #[test]
    fn rejects_unknown_key() {
        let err = parse_key("PotatoButton").unwrap_err().to_string();
        assert!(err.contains("unknown key name"), "got: {err}");
    }

    #[test]
    fn parses_punctuation_keys() {
        for c in ['-', '=', ',', '.', '/', ';', '\'', '\\', '[', ']', '`'] {
            let name = c.to_string();
            assert!(parse_key(&name).is_ok(), "parse_key({name:?}) failed");
        }
    }

    #[test]
    fn parses_lr_modifier_keys() {
        for n in ["LShift", "RShift", "LControl", "LCtrl", "RControl", "RCtrl", "LAlt", "RAlt"] {
            assert!(parse_key(n).is_ok(), "parse_key({n}) failed");
        }
    }

    #[test]
    fn validate_flags_unknown_key_in_binding() {
        let mut cfg = sample_full_config();
        cfg.buttons.insert("0".into(), ButtonEntry {
            label: "Cross".into(),
            binding: Binding::Key("PotatoButton".into()),
        });
        let err = cfg.validate().unwrap_err();
        let s = format!("{err:#}");
        assert!(s.contains("button 0"), "got: {s}");
        assert!(s.contains("PotatoButton"), "got: {s}");
    }

    fn sample_full_config() -> Config {
        let mut buttons = BTreeMap::new();
        for id in VALID_BUTTON_IDS {
            buttons.insert(id.to_string(), ButtonEntry {
                label: format!("button{id}"),
                binding: Binding::Unbound,
            });
        }
        Config {
            version: 1,
            deadzone: 0.4,
            trigger_threshold: 0.5,
            min_press_ms: [8, 25],
            tick_jitter_ms: [0, 3],
            log_events: false,
            buttons,
            macros: BTreeMap::new(),
            touchpad_cursor_enabled: true,
            touchpad_cursor_sensitivity: 1.5,
            touchpad_midpoint_x: 960,
            touchpad_midpoint_y: 540,
            touchpad_accel_slow_threshold: 5,
            touchpad_accel_fast_threshold: 20,
            touchpad_accel_gain_slow: 0.5,
            touchpad_accel_gain_fast: 1.5,
            touchpad_deadzone_radius: 2,
            touchpad_click_freeze_enabled: true,
            mapping_enabled: Default::default(),
        }
    }

    #[test]
    fn binding_mouse_left_round_trips_json() {
        let json = r#"{ "label": "Touchpad TL", "type": "mouse", "value": "left" }"#;
        let entry: ButtonEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.binding, Binding::Mouse(MouseButton::Left));
        let back = serde_json::to_string(&entry).unwrap();
        assert!(back.contains(r#""type":"mouse""#), "got: {back}");
        assert!(back.contains(r#""value":"left""#), "got: {back}");
    }

    #[test]
    fn binding_mouse_wheel_up_kebab_case() {
        let json = r#"{ "label": "L2", "type": "mouse", "value": "wheel-up" }"#;
        let entry: ButtonEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.binding, Binding::Mouse(MouseButton::WheelUp));
    }

    #[test]
    fn config_touchpad_fields_default_when_absent() {
        let mut v: serde_json::Value = serde_json::from_str(r#"{
            "version": 1,
            "deadzone": 0.4,
            "trigger_threshold": 0.5,
            "min_press_ms": [8, 25],
            "tick_jitter_ms": [0, 3],
            "log_events": false,
            "buttons": {"0":{"label":"x","type":"unbound"}},
            "macros": {}
        }"#).unwrap();
        for id in 1..=24u32 {
            v["buttons"][id.to_string()] =
                serde_json::json!({"label": "", "type": "unbound"});
        }
        let s = serde_json::to_string(&v).unwrap();
        let cfg = Config::load_from_str(&s).expect("v2.0-style config must load");
        assert!(cfg.touchpad_cursor_enabled);
        assert!((cfg.touchpad_cursor_sensitivity - 1.5).abs() < 1e-6);
        for id in TOUCHPAD_QUADRANT_IDS {
            assert!(cfg.buttons.contains_key(&id.to_string()),
                "touchpad id {id} must be auto-filled");
        }
    }

    #[test]
    fn v2_0_config_missing_touchpad_ids_auto_migrates_on_load() {
        let cfg = sample_full_config_v20();
        let s = serde_json::to_string(&cfg).unwrap();
        let loaded = Config::load_from_str(&s).expect("v2.0 config must load via auto-fill");
        for id in TOUCHPAD_QUADRANT_IDS {
            match &loaded.buttons[&id.to_string()].binding {
                Binding::Unbound => {}
                other => panic!("auto-filled id {id} should be Unbound, got {other:?}"),
            }
        }
    }

    /// A v2.0-shaped Config with buttons only 0..=28 (no touchpad ids).
    fn sample_full_config_v20() -> Config {
        let mut buttons = BTreeMap::new();
        for id in 0u32..=24 {
            buttons.insert(id.to_string(), ButtonEntry {
                label: format!("button{id}"),
                binding: Binding::Unbound,
            });
        }
        Config {
            version: 1,
            deadzone: 0.4,
            trigger_threshold: 0.5,
            min_press_ms: [8, 25],
            tick_jitter_ms: [0, 3],
            log_events: false,
            buttons,
            macros: BTreeMap::new(),
            touchpad_cursor_enabled: true,
            touchpad_cursor_sensitivity: 1.5,
            touchpad_midpoint_x: 960,
            touchpad_midpoint_y: 540,
            touchpad_accel_slow_threshold: 5,
            touchpad_accel_fast_threshold: 20,
            touchpad_accel_gain_slow: 0.5,
            touchpad_accel_gain_fast: 1.5,
            touchpad_deadzone_radius: 2,
            touchpad_click_freeze_enabled: true,
            mapping_enabled: Default::default(),
        }
    }

    #[test]
    fn rejects_sensitivity_out_of_range() {
        let mut cfg = sample_full_config();
        cfg.touchpad_cursor_sensitivity = 0.0;
        let err = cfg.validate().unwrap_err().to_string();
        assert!(err.contains("touchpad_cursor_sensitivity"), "got: {err}");

        cfg.touchpad_cursor_sensitivity = 20.0;
        let err = cfg.validate().unwrap_err().to_string();
        assert!(err.contains("touchpad_cursor_sensitivity"), "got: {err}");
    }

    #[test]
    fn rejects_slow_geq_fast_accel_threshold() {
        let mut cfg = sample_full_config();
        cfg.touchpad_accel_slow_threshold = 30;
        cfg.touchpad_accel_fast_threshold = 20;
        let err = cfg.validate().unwrap_err().to_string();
        assert!(err.contains("touchpad_accel_slow_threshold"), "got: {err}");
    }

    #[test]
    fn rejects_accel_gain_slow_above_one() {
        let mut cfg = sample_full_config();
        cfg.touchpad_accel_gain_slow = 1.5;
        let err = cfg.validate().unwrap_err().to_string();
        assert!(err.contains("touchpad_accel_gain_slow"), "got: {err}");
    }

    #[test]
    fn rejects_accel_gain_fast_below_one() {
        let mut cfg = sample_full_config();
        cfg.touchpad_accel_gain_fast = 0.8;
        let err = cfg.validate().unwrap_err().to_string();
        assert!(err.contains("touchpad_accel_gain_fast"), "got: {err}");
    }

    #[test]
    fn rejects_deadzone_radius_above_50() {
        let mut cfg = sample_full_config();
        cfg.touchpad_deadzone_radius = 100;
        let err = cfg.validate().unwrap_err().to_string();
        assert!(err.contains("touchpad_deadzone_radius"), "got: {err}");
    }

    #[test]
    fn v21_fix4_config_missing_filter_fields_auto_fills_defaults() {
        // v2.0 / v2.1.0-fix4 style: existing touchpad fields present, new
        // cursor filter fields absent. Must load via #[serde(default)].
        let mut v: serde_json::Value = serde_json::from_str(r#"{
            "version": 1,
            "deadzone": 0.4,
            "trigger_threshold": 0.5,
            "min_press_ms": [8, 25],
            "tick_jitter_ms": [0, 3],
            "log_events": false,
            "buttons": {},
            "macros": {},
            "touchpad_cursor_enabled": true,
            "touchpad_cursor_sensitivity": 1.5,
            "touchpad_midpoint_x": 960,
            "touchpad_midpoint_y": 540
        }"#).unwrap();
        for id in 0u32..=28 {
            v["buttons"][id.to_string()] =
                serde_json::json!({"label": "", "type": "unbound"});
        }
        let s = serde_json::to_string(&v).unwrap();
        let cfg = Config::load_from_str(&s).expect("fix4-style config must load");
        assert_eq!(cfg.touchpad_accel_slow_threshold, 5);
        assert_eq!(cfg.touchpad_accel_fast_threshold, 20);
        assert!((cfg.touchpad_accel_gain_slow - 0.5).abs() < 1e-6);
        assert!((cfg.touchpad_accel_gain_fast - 1.5).abs() < 1e-6);
        assert_eq!(cfg.touchpad_deadzone_radius, 2);
        assert!(cfg.touchpad_click_freeze_enabled);
    }

    fn minimal_json_with(extra: &str) -> String {
        let mut v: serde_json::Value = serde_json::from_str(r#"{
            "version": 1, "deadzone": 0.4, "trigger_threshold": 0.5,
            "min_press_ms": [8, 25], "tick_jitter_ms": [0, 3],
            "log_events": false, "buttons": {}, "macros": {}
        }"#).unwrap();
        for id in 0u32..=28 {
            v["buttons"][id.to_string()] = serde_json::json!({"label": "", "type": "unbound"});
        }
        if !extra.is_empty() {
            let e: serde_json::Value = serde_json::from_str(extra).unwrap();
            v["mapping_enabled"] = e;
        }
        serde_json::to_string(&v).unwrap()
    }

    #[test]
    fn mapping_enabled_missing_uses_defaults() {
        use crate::gamepad::ControllerKind;
        let cfg = Config::load_from_str(&minimal_json_with("")).unwrap();
        assert!(cfg.mapping_enabled_for(ControllerKind::DualSense));
        assert!(!cfg.mapping_enabled_for(ControllerKind::Ns2Pro));
    }

    #[test]
    fn mapping_enabled_explicit_value_wins() {
        use crate::gamepad::ControllerKind;
        let cfg = Config::load_from_str(&minimal_json_with(r#"{"dualsense": false}"#)).unwrap();
        assert!(!cfg.mapping_enabled_for(ControllerKind::DualSense));
    }

    #[test]
    fn validate_rejects_unknown_mapping_slug() {
        let mut cfg = Config::load_from_str(&minimal_json_with("")).unwrap();
        cfg.mapping_enabled.insert("ps5".to_string(), true);
        let err = cfg.validate().unwrap_err().to_string();
        assert!(err.contains("ps5") && err.contains("mapping_enabled"), "got: {err}");
    }
}
