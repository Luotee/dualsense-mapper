//! User stick calibration (spec 5.5), applied on top of each pad's factory
//! calibration in the gamepad layer (iron rule 6).
//!
//! Per axis, a correction of the already-normalized `[-1, 1]` value:
//! `d = v - center; out = d >= 0 ? d / pos : d / neg`, clamped to `[-1, 1]`.
//! `check` holds the reject rules; the config validator and the save IPC both
//! use it, so a bad capture never reaches the file.

use serde::{Deserialize, Serialize};

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
