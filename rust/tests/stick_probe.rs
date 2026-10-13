//! `StickProbe` keeps the per-axis extents of raw stick values seen between
//! two reads, so a fast rotation that passes the rim between 30 Hz polls of
//! `get_stick_positions` still reaches the capture (v3.1.2).

use dualsense_mapper::gamepad::controller::ControllerKind;
use dualsense_mapper::gamepad::stick_cal::{StickPositions, StickProbe};

fn pos(raw: [f32; 4]) -> StickPositions {
    StickPositions { pad: ControllerKind::DualSense, raw, cal: raw, raw_min: raw, raw_max: raw }
}

#[test]
fn extents_cover_values_between_takes() {
    let probe = StickProbe::default();
    probe.set(Some(pos([0.0, 0.0, 0.0, 0.0])));
    probe.set(Some(pos([0.9, -0.2, 0.1, 0.0])));
    probe.set(Some(pos([0.1, 0.5, -0.7, 0.3])));
    let p = probe.take().expect("positions");
    assert_eq!(p.raw, [0.1, 0.5, -0.7, 0.3]);
    assert_eq!(p.raw_min, [0.0, -0.2, -0.7, 0.0]);
    assert_eq!(p.raw_max, [0.9, 0.5, 0.1, 0.3]);
}

#[test]
fn extents_reset_after_take() {
    let probe = StickProbe::default();
    probe.set(Some(pos([0.0, 0.0, 0.0, 0.0])));
    probe.set(Some(pos([1.0, -1.0, 0.5, -0.5])));
    probe.set(Some(pos([0.2, 0.1, 0.0, 0.0])));
    let _ = probe.take();
    let p = probe.take().expect("positions");
    assert_eq!(p.raw_min, p.raw);
    assert_eq!(p.raw_max, p.raw);
    // A value after the take widens only the new window.
    probe.set(Some(pos([-0.4, 0.1, 0.0, 0.0])));
    let p = probe.take().unwrap();
    assert_eq!(p.raw_min[0], -0.4);
    assert_eq!(p.raw_max[0], 0.2);
}

#[test]
fn pad_change_restarts_extents() {
    let probe = StickProbe::default();
    probe.set(Some(pos([1.0, 1.0, 1.0, 1.0])));
    let other = StickPositions { pad: ControllerKind::Ns2Pro, raw: [0.0; 4], cal: [0.0; 4], raw_min: [0.0; 4], raw_max: [0.0; 4] };
    probe.set(Some(other));
    let p = probe.take().unwrap();
    assert_eq!(p.pad, ControllerKind::Ns2Pro);
    assert_eq!(p.raw_max, [0.0; 4]);
}

#[test]
fn cleared_probe_takes_none() {
    let probe = StickProbe::default();
    probe.set(Some(pos([0.3; 4])));
    probe.set(None);
    assert!(probe.take().is_none());
}
