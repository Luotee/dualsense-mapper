//! Rumble-on-connect: pure frame builders and the buzz/stop timeline.
//!
//! No I/O here. The byte layouts are facts taken from the research file
//! docs (DualSense BT output report 0x31, Switch Pro output report 0x10,
//! Switch 2 Pro vibration characteristic); no code was copied from any
//! source. The workers own the writes and the clock; this module only says
//! what to send and when.

use std::time::{Duration, Instant};

use super::ControllerKind;

/// One action on the rumble timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Buzz,
    Stop,
}

/// A fixed buzz-then-stop timeline for one pad, driven by the caller's clock.
#[derive(Debug)]
pub struct RumblePlan {
    start: Instant,
    steps: Vec<(u64, Step)>,
    sent: usize,
}

impl RumblePlan {
    pub fn new(kind: ControllerKind, start: Instant) -> Self {
        let mut steps: Vec<(u64, Step)> = Vec::new();
        match kind {
            // Stop is sent twice: a lost stop would leave the pad buzzing.
            ControllerKind::DualSense => {
                steps.push((0, Step::Buzz));
                steps.extend([(200, Step::Stop), (230, Step::Stop)]);
            }
            // Writes stay >= 30 ms apart (Switch Pro drops faster ones).
            ControllerKind::NsPro => {
                steps.extend((0..4).map(|i| (i * 50, Step::Buzz)));
                steps.extend([(200, Step::Stop), (240, Step::Stop), (280, Step::Stop)]);
            }
            // The vibration characteristic wants a ~12 ms frame stream.
            ControllerKind::Ns2Pro => {
                steps.extend((0..17).map(|i| (i * 12, Step::Buzz)));
                steps.extend([(204, Step::Stop), (216, Step::Stop), (228, Step::Stop)]);
            }
        }
        Self { start, steps, sent: 0 }
    }

    /// The first unsent step whose time is `<= now`.
    pub fn due(&self, now: Instant) -> Option<Step> {
        let &(ms, step) = self.steps.get(self.sent)?;
        (now >= self.start + Duration::from_millis(ms)).then_some(step)
    }

    /// Mark the step `due` returned as sent.
    pub fn advance(&mut self) {
        if self.sent < self.steps.len() {
            self.sent += 1;
        }
    }

    pub fn next_due(&self) -> Option<Instant> {
        self.steps.get(self.sent).map(|&(ms, _)| self.start + Duration::from_millis(ms))
    }

    pub fn finished(&self) -> bool {
        self.sent >= self.steps.len()
    }

    /// Cut the timeline short. `Some(Stop)` if a buzz went out and no stop
    /// has yet, so the caller must send one; the plan is finished after.
    pub fn abort(&mut self) -> Option<Step> {
        let buzzing = self.steps[..self.sent].iter().any(|s| s.1 == Step::Buzz)
            && !self.steps[..self.sent].iter().any(|s| s.1 == Step::Stop);
        self.sent = self.steps.len();
        buzzing.then_some(Step::Stop)
    }
}

/// Both DualSense motors at 25 %: gentle but clearly felt.
pub const DS_BUZZ_MOTOR: u8 = 0x40;

/// DualSense BT output report 0x31 (78 bytes) that only touches the motors.
/// LED, audio and trigger flags stay 0 so those states are left alone.
pub fn ds_report(motor: u8, v2: bool) -> [u8; 78] {
    let mut r = [0u8; 78];
    r[0] = 0x31;
    r[2] = 0x10;
    // New firmware: HAPTICS_SELECT + VIBRATION2. Old: plus COMPATIBLE_VIBRATION.
    r[3] = if v2 { 0x02 } else { 0x03 };
    r[5] = motor;
    r[6] = motor;
    if v2 {
        r[41] = 0x04;
    }
    let mut h = crc32fast::Hasher::new();
    h.update(&[0xA2]);
    h.update(&r[..74]);
    r[74..78].copy_from_slice(&h.finalize().to_le_bytes());
    r
}

/// Firmware "update version" is a u16 LE at bytes 44..45 of feature report
/// 0x20; v2 rumble from 0x0224. Unreadable or short counts as new.
pub fn ds_firmware_is_v2(feature_0x20: Option<&[u8]>) -> bool {
    match feature_0x20 {
        Some(b) if b.len() >= 46 => u16::from_le_bytes([b[44], b[45]]) >= 0x0224,
        _ => true,
    }
}

/// Switch Pro rumble payload for a moderate buzz (both motors).
pub const NS_BUZZ: [u8; 8] = [0x00, 0x75, 0x40, 0x5d, 0x00, 0x75, 0x40, 0x5d];

/// Switch Pro output report 0x10 (rumble only), padded to the 49-byte length.
pub fn ns_rumble_report(counter: u8, motors: [u8; 8]) -> [u8; 49] {
    let mut r = [0u8; 49];
    r[0] = 0x10;
    r[1] = counter & 0x0F;
    r[2..10].copy_from_slice(&motors);
    r
}

/// Switch 2 Pro vibration opcodes (5 bytes, repeated in both halves).
pub const NS2_BUZZ_OP: [u8; 5] = [0x87, 0x21, 0x23, 0x11, 0x32];
pub const NS2_STOP_OP: [u8; 5] = [0x87, 0x01, 0x20, 0x11, 0x00];

/// 42-byte write to the Switch 2 Pro vibration characteristic: the same
/// `0x50 | seq` + opcode record at offsets 0x01 and 0x11.
pub fn ns2_vibration_frame(seq: u8, op: [u8; 5]) -> [u8; 42] {
    let mut f = [0u8; 42];
    for at in [0x01usize, 0x11] {
        f[at] = 0x50 | (seq & 0x0F);
        f[at + 1..at + 6].copy_from_slice(&op);
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gamepad::ControllerKind;
    use std::time::{Duration, Instant};

    fn crc(b: &[u8]) -> [u8; 4] { b[74..78].try_into().unwrap() }

    /// Drive a plan with a fake clock, collect (ms, step).
    fn run(kind: ControllerKind) -> Vec<(u64, Step)> {
        let t0 = Instant::now();
        let mut p = RumblePlan::new(kind, t0);
        let mut out = vec![];
        for ms in 0..400u64 {
            while let Some(s) = p.due(t0 + Duration::from_millis(ms)) { out.push((ms, s)); p.advance(); }
        }
        assert!(p.finished());
        out
    }

    #[test] fn ds_v2_buzz_matches_vector() {
        let r = ds_report(DS_BUZZ_MOTOR, true);
        assert_eq!((r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[41]), (0x31, 0, 0x10, 0x02, 0, 0x40, 0x40, 0x04));
        assert_eq!(crc(&r), [0xd6, 0x65, 0x7a, 0x40]);
    }
    #[test] fn ds_v2_stop_matches_vector() { assert_eq!(crc(&ds_report(0, true)), [0xe8, 0xf9, 0xdd, 0x97]); }
    #[test] fn ds_old_buzz_matches_vector() {
        let r = ds_report(DS_BUZZ_MOTOR, false);
        assert_eq!((r[3], r[5], r[6], r[41]), (0x03, 0x40, 0x40, 0x00)); // no halving in old mode
        assert_eq!(crc(&r), [0xf2, 0xf8, 0x89, 0x41]);
    }
    #[test] fn ds_old_stop_matches_vector() { assert_eq!(crc(&ds_report(0, false)), [0xcc, 0x64, 0x2e, 0x96]); }
    #[test] fn ds_report_leaves_leds_and_triggers_alone() {
        let r = ds_report(DS_BUZZ_MOTOR, true);
        assert!(r[7..41].iter().all(|&b| b == 0) && r[42..74].iter().all(|&b| b == 0) && r[4] == 0);
    }
    #[test] fn ds_firmware_threshold_follows_sdl() {
        let fw = |v: u16| { let mut b = vec![0u8; 64]; b[0] = 0x20; b[44..46].copy_from_slice(&v.to_le_bytes()); b };
        assert!(!ds_firmware_is_v2(Some(&fw(0x0223))));
        assert!(ds_firmware_is_v2(Some(&fw(0x0224))));
        assert!(ds_firmware_is_v2(None));
        assert!(ds_firmware_is_v2(Some(&[0x20; 40])));  // short read
    }
    #[test] fn ns_rumble_report_layout() {
        let r = ns_rumble_report(0x0B, NS_BUZZ);
        assert_eq!(&r[..10], &[0x10, 0x0B, 0x00, 0x75, 0x40, 0x5d, 0x00, 0x75, 0x40, 0x5d]);
        assert!(r[10..].iter().all(|&b| b == 0));
        assert_eq!(&ns_rumble_report(3, crate::gamepad::ns_protocol::NEUTRAL_RUMBLE)[2..10],
                   &[0x00, 0x01, 0x40, 0x40, 0x00, 0x01, 0x40, 0x40]);
    }
    #[test] fn ns2_frame_matches_spike_dry_run() {
        let f = ns2_vibration_frame(1, NS2_BUZZ_OP);
        let mut want = [0u8; 42];
        want[0x01..0x07].copy_from_slice(&[0x51, 0x87, 0x21, 0x23, 0x11, 0x32]);
        want[0x11..0x17].copy_from_slice(&[0x51, 0x87, 0x21, 0x23, 0x11, 0x32]);
        assert_eq!(f, want);
        assert_eq!(ns2_vibration_frame(0x13, NS2_STOP_OP)[1], 0x53); // seq wraps into the low nibble
    }
    #[test] fn plan_timelines_fire_each_step_once() {
        use Step::*;
        assert_eq!(run(ControllerKind::DualSense), vec![(0, Buzz), (200, Stop), (230, Stop)]);
        assert_eq!(run(ControllerKind::NsPro),
            vec![(0, Buzz), (50, Buzz), (100, Buzz), (150, Buzz), (200, Stop), (240, Stop), (280, Stop)]);
        let ns2 = run(ControllerKind::Ns2Pro);
        assert_eq!(ns2.iter().filter(|s| s.1 == Buzz).count(), 17);
        assert_eq!(ns2[16], (192, Buzz));
        assert_eq!(&ns2[17..], &[(204, Stop), (216, Stop), (228, Stop)]);
    }
    #[test] fn ns_pro_steps_are_at_least_30ms_apart() {
        let steps = run(ControllerKind::NsPro);
        assert!(steps.len() > 1);
        for w in steps.windows(2) {
            assert!(w[1].0 - w[0].0 >= 30, "gap {} -> {} under 30 ms", w[0].0, w[1].0);
        }
    }
    #[test] fn abort_returns_stop_only_while_buzzing() {
        let t0 = Instant::now();
        let mut p = RumblePlan::new(ControllerKind::DualSense, t0);
        assert_eq!(p.abort(), None);                       // nothing sent yet
        let mut p = RumblePlan::new(ControllerKind::DualSense, t0);
        p.due(t0).unwrap(); p.advance();                   // Buzz sent
        assert_eq!(p.abort(), Some(Step::Stop));
        assert!(p.finished());
        let mut p = RumblePlan::new(ControllerKind::DualSense, t0);
        while let Some(_) = p.due(t0 + Duration::from_millis(500)) { p.advance(); }
        assert_eq!(p.abort(), None);                       // already stopped
    }
    #[test] fn next_due_tracks_the_next_unsent_step() {
        let t0 = Instant::now();
        let mut p = RumblePlan::new(ControllerKind::NsPro, t0);
        assert_eq!(p.next_due(), Some(t0));
        p.due(t0).unwrap(); p.advance();
        assert_eq!(p.next_due(), Some(t0 + Duration::from_millis(50)));
    }
}
