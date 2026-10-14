//! User stick calibration (spec 5.5), applied on top of each pad's factory
//! calibration in the gamepad layer (iron rule 6).
//!
//! Per axis, a correction of the already-normalized `[-1, 1]` value:
//! `d = v - center; out = d >= 0 ? d / pos : d / neg`, clamped to `[-1, 1]`.
//! `check` holds the reject rules; the config validator and the save IPC both
//! use it, so a bad capture never reaches the file.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use super::controller::ControllerKind;

/// Largest accepted rest offset from centre, per axis.
pub const MAX_CENTER: f32 = 0.25;
/// Accepted range of each extent (distance from centre to full deflection).
pub const EXTENT_RANGE: (f32, f32) = (0.4, 1.25);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AxisCal {
    pub center: f32,
    pub neg: f32,
    pub pos: f32,
}

impl AxisCal {
    pub const IDENTITY: AxisCal = AxisCal { center: 0.0, neg: 1.0, pos: 1.0 };

    pub fn apply(&self, v: f32) -> f32 {
        if *self == Self::IDENTITY {
            return v;
        }
        let d = v - self.center;
        let out = if d >= 0.0 { d / self.pos } else { d / self.neg };
        out.clamp(-1.0, 1.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StickCal {
    pub x: AxisCal,
    pub y: AxisCal,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PadStickCal {
    pub left: StickCal,
    pub right: StickCal,
}

impl PadStickCal {
    /// The correction of stick axis `axis` (0 lx, 1 ly, 2 rx, 3 ry).
    pub fn axis(&self, axis: u32) -> &AxisCal {
        match axis {
            0 => &self.left.x,
            1 => &self.left.y,
            2 => &self.right.x,
            _ => &self.right.y,
        }
    }
}

/// The live corrections, shared by the engine (writer, from the config) and
/// the hub (reader, per stick event).
#[derive(Clone, Default)]
pub struct StickCalTable(Arc<RwLock<HashMap<ControllerKind, PadStickCal>>>);

impl StickCalTable {
    pub fn get(&self, kind: ControllerKind) -> Option<PadStickCal> {
        self.0.read().unwrap_or_else(|p| p.into_inner()).get(&kind).copied()
    }

    pub fn set(&self, kind: ControllerKind, cal: Option<PadStickCal>) {
        let mut t = self.0.write().unwrap_or_else(|p| p.into_inner());
        match cal {
            Some(c) => { t.insert(kind, c); }
            None => { t.remove(&kind); }
        }
    }

    /// Mirror `cfg.stick_cal` (unknown slugs are ignored; validate rejects them).
    pub fn replace_from(&self, cfg: &crate::config::Config) {
        let next: HashMap<ControllerKind, PadStickCal> = cfg
            .stick_cal
            .iter()
            .filter_map(|(slug, c)| ControllerKind::from_slug(slug).map(|k| (k, *c)))
            .collect();
        *self.0.write().unwrap_or_else(|p| p.into_inner()) = next;
    }
}

/// The session owner's latest stick positions, raw (factory calibration
/// only) and corrected, for the 設定 → 搖桿 view and the capture.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct StickPositions {
    pub pad: ControllerKind,
    /// lx, ly, rx, ry
    pub raw: [f32; 4],
    pub cal: [f32; 4],
    /// Per-axis min / max of `raw` seen since the previous [`StickProbe::take`]
    /// (lx ly rx ry). A fast rotation can pass the rim between two polls of
    /// the capture; the extents still carry it (v3.1.2).
    pub raw_min: [f32; 4],
    pub raw_max: [f32; 4],
}

impl StickPositions {
    /// Positions with the extents collapsed onto `raw`.
    pub fn at(pad: ControllerKind, raw: [f32; 4], cal: [f32; 4]) -> Self {
        StickPositions { pad, raw, cal, raw_min: raw, raw_max: raw }
    }
}

#[derive(Clone, Default)]
pub struct StickProbe(Arc<Mutex<Option<StickPositions>>>);

impl StickProbe {
    /// The latest positions, extents untouched.
    pub fn snapshot(&self) -> Option<StickPositions> {
        *self.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The latest positions with the extents seen since the previous `take`,
    /// then restart the extents at the current raw values.
    pub fn take(&self) -> Option<StickPositions> {
        let mut g = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let out = *g;
        if let Some(p) = g.as_mut() {
            p.raw_min = p.raw;
            p.raw_max = p.raw;
        }
        out
    }

    /// Record new positions, folding `raw` into the running extents (the
    /// incoming `raw_min` / `raw_max` are ignored). A different pad, or
    /// `None`, restarts them.
    pub fn set(&self, p: Option<StickPositions>) {
        let mut g = self.0.lock().unwrap_or_else(|e| e.into_inner());
        *g = match (p, g.as_ref()) {
            (Some(mut n), Some(old)) if old.pad == n.pad => {
                for i in 0..4 {
                    n.raw_min[i] = old.raw_min[i].min(n.raw[i]);
                    n.raw_max[i] = old.raw_max[i].max(n.raw[i]);
                }
                Some(n)
            }
            (Some(n), _) => Some(StickPositions::at(n.pad, n.raw, n.cal)),
            (None, _) => None,
        };
    }
}

/// The reject rules. The message names the stick and the axis.
pub fn check(cal: &PadStickCal) -> Result<(), String> {
    for (stick, sc) in [("左搖桿", &cal.left), ("右搖桿", &cal.right)] {
        for (axis, a) in [("X", &sc.x), ("Y", &sc.y)] {
            if !(a.center.abs() <= MAX_CENTER) {
                return Err(format!("{stick} {axis}：中心偏太多（{:.2}）", a.center));
            }
            for e in [a.neg, a.pos] {
                if !(e >= EXTENT_RANGE.0 && e <= EXTENT_RANGE.1) {
                    return Err(format!("{stick} {axis}：推不到底或範圍不對（{e:.2}）"));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ax(center: f32, neg: f32, pos: f32) -> AxisCal { AxisCal { center, neg, pos } }
    fn pad(x: AxisCal) -> PadStickCal {
        PadStickCal { left: StickCal { x, y: AxisCal::IDENTITY }, right: StickCal { x: AxisCal::IDENTITY, y: AxisCal::IDENTITY } }
    }

    #[test]
    fn apply_maps_center_and_extents() {
        let a = ax(0.1, 0.8, 0.6);
        let close = |v: f32, w: f32| assert!((a.apply(v) - w).abs() < 1e-5, "{v} -> {} want {w}", a.apply(v));
        close(0.1, 0.0);
        close(0.7, 1.0);
        close(-0.7, -1.0);
        close(1.0, 1.0);
        close(0.4, 0.5);
        close(-1.0, -1.0);
    }

    #[test]
    fn identity_is_noop() {
        for v in [-1.0, -0.3, 0.0, 0.42, 1.0] {
            assert_eq!(AxisCal::IDENTITY.apply(v), v);
        }
    }

    #[test]
    fn check_rejects_center_and_extent() {
        assert!(check(&pad(ax(0.1, 0.8, 0.6))).is_ok());
        let e = check(&pad(ax(0.3, 0.8, 0.8))).unwrap_err();
        assert!(e.contains("左搖桿") && e.contains('X') && e.contains("中心"), "{e}");
        let e = check(&pad(ax(0.0, 0.8, 0.3))).unwrap_err();
        assert!(e.contains("推不到底"), "{e}");
        let e = check(&pad(ax(0.0, 1.3, 0.8))).unwrap_err();
        assert!(e.contains("左搖桿 X"), "{e}");
        assert!(check(&pad(ax(f32::NAN, 1.0, 1.0))).is_err());
    }

    #[test]
    fn axis_index_maps_lx_ly_rx_ry() {
        let p = PadStickCal {
            left: StickCal { x: ax(0.01, 1.0, 1.0), y: ax(0.02, 1.0, 1.0) },
            right: StickCal { x: ax(0.03, 1.0, 1.0), y: ax(0.04, 1.0, 1.0) },
        };
        for (i, c) in [0.01, 0.02, 0.03, 0.04].into_iter().enumerate() {
            assert_eq!(p.axis(i as u32).center, c);
        }
    }
}
