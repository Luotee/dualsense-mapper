//! Controller kinds the mapper can drive. Standalone: no dependency on
//! `config` so the gamepad layer stays self-contained.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub enum ControllerKind {
    #[serde(rename = "dualsense")]
    DualSense,
    #[serde(rename = "ns_pro")]
    NsPro,
    #[serde(rename = "ns2_pro")]
    Ns2Pro,
}

/// Button ids 0..=28 (the full DualSense surface).
const DUALSENSE_CAPS: [u32; 29] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
    26, 27, 28,
];

/// Switch Pro: 0..=24 (no touchpad quadrants 25..=28) plus 29 (Capture).
const NS_PRO_CAPS: [u32; 26] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 29,
];

impl ControllerKind {
    pub const ALL: [ControllerKind; 3] = [
        ControllerKind::DualSense,
        ControllerKind::NsPro,
        ControllerKind::Ns2Pro,
    ];

    pub fn slug(self) -> &'static str {
        match self {
            ControllerKind::DualSense => "dualsense",
            ControllerKind::NsPro => "ns_pro",
            ControllerKind::Ns2Pro => "ns2_pro",
        }
    }

    pub fn from_slug(s: &str) -> Option<ControllerKind> {
        Self::ALL.into_iter().find(|k| k.slug() == s)
    }

    pub fn display_name(self) -> &'static str {
        match self {
            ControllerKind::DualSense => "DualSense",
            ControllerKind::NsPro => "Switch Pro Controller",
            ControllerKind::Ns2Pro => "Switch 2 Pro Controller",
        }
    }

    pub fn capabilities(self) -> &'static [u32] {
        match self {
            ControllerKind::DualSense => &DUALSENSE_CAPS,
            ControllerKind::NsPro => &NS_PRO_CAPS,
            // Filled in by PR 3.
            ControllerKind::Ns2Pro => &[],
        }
    }

    pub fn default_mapping_enabled(self) -> bool {
        !matches!(self, ControllerKind::Ns2Pro)
    }

    pub fn needs_virtual_pad(self) -> bool {
        matches!(self, ControllerKind::Ns2Pro)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_roundtrip() {
        for k in ControllerKind::ALL {
            assert_eq!(ControllerKind::from_slug(k.slug()), Some(k));
        }
        assert_eq!(ControllerKind::from_slug("ps5"), None);
    }

    #[test]
    fn defaults_match_spec() {
        assert!(ControllerKind::DualSense.default_mapping_enabled());
        assert!(ControllerKind::NsPro.default_mapping_enabled());
        assert!(!ControllerKind::Ns2Pro.default_mapping_enabled());
    }

    #[test]
    fn only_ns2_needs_virtual_pad() {
        assert!(!ControllerKind::DualSense.needs_virtual_pad());
        assert!(!ControllerKind::NsPro.needs_virtual_pad());
        assert!(ControllerKind::Ns2Pro.needs_virtual_pad());
    }

    #[test]
    fn ns_pro_capabilities() {
        let mut want: Vec<u32> = (0..=24).collect();
        want.push(29);
        assert_eq!(ControllerKind::NsPro.capabilities(), want);
        assert_eq!(ControllerKind::NsPro.capabilities().len(), 26);
    }

    #[test]
    fn dualsense_capabilities_are_0_to_28() {
        assert_eq!(
            ControllerKind::DualSense.capabilities(),
            (0..=28).collect::<Vec<u32>>()
        );
    }

    #[test]
    fn serializes_as_slug() {
        assert_eq!(
            serde_json::to_string(&ControllerKind::NsPro).unwrap(),
            "\"ns_pro\""
        );
    }
}
