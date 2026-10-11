//! Switch Pro Controller handshake over classic Bluetooth HID.
//!
//! Sequence, each subcommand answered by a 0x21 reply:
//! 1. SPI read of the user stick calibration (0x8010, 0x16 bytes); when it
//!    lacks the `B2 A1` magic, SPI read of the factory one (0x603D, 0x12);
//! 2. `0x03 0x30` — switch the pad to the full 0x30 input report;
//! 3. `0x30 0x01` — player LED 1;
//! 4. `0x48 0x01` — enable vibration (for the rumble on connect).
//!
//! The pad may already stream 0x30 / 0x3F reports while we wait, so an ACK
//! wait skips everything that is not the 0x21 reply for the subcommand just
//! sent. A pad whose ACKs never arrive (e.g. held by Steam) costs at most
//! three attempts per subcommand: the handshake never fails, it logs and
//! falls back to default calibration. Sleep is injected so tests run without
//! wall-clock waits.

use super::ds_protocol::REPORT_LEN_BT;
use super::hid_source::HidIo;
use super::ns_protocol::{
    build_subcmd, parse_21, parse_factory_cal, parse_user_cal, spi_read_args, StickCal,
};
use std::time::{Duration, Instant};

/// Minimum spacing between two subcommand writes.
const SUBCMD_GAP: Duration = Duration::from_millis(60);
/// How long one attempt waits for its ACK.
const ACK_WAIT: Duration = Duration::from_millis(100);
/// Read timeout while waiting for an ACK.
const ACK_READ_MS: i32 = 10;
/// Hard cap on reads per ACK wait, in case the pad floods other reports.
const ACK_MAX_READS: usize = 64;
/// One send plus at most two retries.
const ATTEMPTS: usize = 3;

const SUBCMD_SPI_READ: u8 = 0x10;
const SUBCMD_INPUT_MODE: u8 = 0x03;
const SUBCMD_PLAYER_LED: u8 = 0x30;
const SUBCMD_TURN_OFF: u8 = 0x06;
const SUBCMD_ENABLE_VIBRATION: u8 = 0x48;
const TURN_OFF_ARG: u8 = 0x00;
const INPUT_MODE_FULL: u8 = 0x30;
const PLAYER_LED_1: u8 = 0x01;
const USER_CAL: (u32, u8) = (0x8010, 0x16);
const FACTORY_CAL: (u32, u8) = (0x603D, 0x12);
/// An SPI read reply echoes the address (u32 LE) and length before the bytes.
const SPI_REPLY_HEADER: usize = 5;

/// Run the Switch Pro handshake with a fresh subcommand counter.
pub fn ns_handshake(io: &dyn HidIo, sleep: &mut dyn FnMut(Duration)) -> StickCal {
    let mut counter = 0;
    ns_handshake_counted(io, sleep, &mut counter)
}

/// Same as [`ns_handshake`], on the device session's subcommand counter so
/// later writes (the 0x3F re-init) continue the sequence.
pub(crate) fn ns_handshake_counted(
    io: &dyn HidIo,
    sleep: &mut dyn FnMut(Duration),
    counter: &mut u8,
) -> StickCal {
    let mut link = Link { io, sleep, counter, wrote: false };
    let cal = match link.spi_read(USER_CAL).and_then(|b| parse_user_cal(&b)) {
        Some(c) => {
            tracing::info!("Switch Pro: using user stick calibration");
            c
        }
        None => match link.spi_read(FACTORY_CAL) {
            Some(b) => parse_factory_cal(&b),
            None => {
                tracing::warn!("Switch Pro: no calibration read; using defaults");
                StickCal::default()
            }
        },
    };
    if link.request(SUBCMD_INPUT_MODE, &[INPUT_MODE_FULL], |_| true).is_none() {
        tracing::warn!("Switch Pro: input mode 0x30 not acknowledged");
    }
    if link.request(SUBCMD_PLAYER_LED, &[PLAYER_LED_1], |_| true).is_none() {
        tracing::warn!("Switch Pro: player LED not acknowledged");
    }
    if link.request(SUBCMD_ENABLE_VIBRATION, &[0x01], |_| true).is_none() {
        tracing::warn!("Switch Pro: enable vibration not acknowledged");
    }
    cal
}

/// Fire-and-forget `0x03 0x30` (back to the full 0x30 report), used when
/// the pad falls back to 0x3F while streaming. The reply is not awaited.
pub(crate) fn send_input_mode_full(io: &dyn HidIo, counter: &mut u8) {
    write_subcmd(io, counter, SUBCMD_INPUT_MODE, &[INPUT_MODE_FULL]);
}

/// Fire-and-forget `0x06 0x00` (HCI "sleep" / Turn Off), sent before the
/// handle is dropped. The ACK is not awaited: the pad is about to go away.
pub(crate) fn send_turn_off(io: &dyn HidIo, counter: &mut u8) {
    write_subcmd(io, counter, SUBCMD_TURN_OFF, &[TURN_OFF_ARG]);
}

/// Write one subcommand and advance the counter (mod 16). Logs failures.
fn write_subcmd(io: &dyn HidIo, counter: &mut u8, id: u8, args: &[u8]) -> bool {
    let report = build_subcmd(*counter, id, args);
    *counter = counter.wrapping_add(1) & 0x0F;
    match io.write(&report) {
        Ok(_) => true,
        Err(e) => {
            tracing::warn!(error = %e, id, "Switch Pro: subcommand write failed");
            false
        }
    }
}

struct Link<'a> {
    io: &'a dyn HidIo,
    sleep: &'a mut dyn FnMut(Duration),
    counter: &'a mut u8,
    /// Whether a write already happened (the gap applies between writes).
    wrote: bool,
}

impl Link<'_> {
    /// Send `id args` until an ACKed reply that `accept` takes arrives, at
    /// most `ATTEMPTS` times. Returns the reply data (from 0x21 byte 15).
    fn request(&mut self, id: u8, args: &[u8], accept: impl Fn(&[u8]) -> bool) -> Option<Vec<u8>> {
        for attempt in 1..=ATTEMPTS {
            if self.wrote {
                (self.sleep)(SUBCMD_GAP);
            }
            self.wrote = true;
            if !write_subcmd(self.io, self.counter, id, args) {
                continue;
            }
            if let Some(data) = self.wait_ack(id, &accept) {
                return Some(data);
            }
            tracing::debug!(id, attempt, "Switch Pro: no ACK");
        }
        tracing::warn!(id, attempts = ATTEMPTS, "Switch Pro: subcommand gave up");
        None
    }

    /// Read until the ACK for `id` arrives, a NACK arrives, or ~100 ms pass.
    /// Time is counted both on the wall clock and in read timeouts, so a
    /// fake device that returns instantly still ends the wait.
    fn wait_ack(&self, id: u8, accept: &impl Fn(&[u8]) -> bool) -> Option<Vec<u8>> {
        let mut buf = [0u8; REPORT_LEN_BT];
        let start = Instant::now();
        let mut idle = Duration::ZERO;
        for _ in 0..ACK_MAX_READS {
            if idle >= ACK_WAIT || start.elapsed() >= ACK_WAIT {
                break;
            }
            match self.io.read_timeout(&mut buf, ACK_READ_MS) {
                Ok(0) => idle += Duration::from_millis(ACK_READ_MS as u64),
                Ok(n) => match parse_21(&buf[..n]) {
                    Some(r) if r.id == id && !r.ack => {
                        tracing::debug!(id, "Switch Pro: NACK");
                        return None;
                    }
                    Some(r) if r.id == id && accept(r.data) => return Some(r.data.to_vec()),
                    // 0x30 / 0x3F input, or a reply to another subcommand.
                    _ => {}
                },
                Err(e) => {
                    tracing::warn!(error = %e, "Switch Pro: read failed during handshake");
                    return None;
                }
            }
        }
        None
    }

    /// SPI flash read; returns exactly `len` bytes from a reply that echoes
    /// the requested address and length.
    fn spi_read(&mut self, (addr, len): (u32, u8)) -> Option<Vec<u8>> {
        let args = spi_read_args(addr, len);
        let len = len as usize;
        let data = self.request(SUBCMD_SPI_READ, &args, |d| {
            d.len() >= SPI_REPLY_HEADER + len && d[..SPI_REPLY_HEADER] == args
        })?;
        Some(data[SPI_REPLY_HEADER..SPI_REPLY_HEADER + len].to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gamepad::hid_source::HidIo;
    use crate::gamepad::ns_protocol::{parse_factory_cal, parse_user_cal, spi_read_args, StickCal};
    use anyhow::Result;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    #[derive(Debug, Clone, PartialEq)]
    enum Ev {
        Write(Vec<u8>),
        Sleep(Duration),
    }

    /// Scripted Switch Pro: records writes on a shared timeline and, when
    /// `respond` is set, queues noise (a 0x30 frame, a 0x3F frame, a 0x21
    /// reply for an unrelated subcmd) followed by the real 0x21 ACK.
    struct FakeNs {
        log: Rc<RefCell<Vec<Ev>>>,
        queue: RefCell<VecDeque<Vec<u8>>>,
        respond: bool,
        /// Subcmd ids whose first attempt is NACKed (ack bit clear).
        nack_first: RefCell<Vec<u8>>,
        /// Subcmd ids that get no reply at all.
        silent: Vec<u8>,
        user_spi: [u8; 0x16],
        factory_spi: [u8; 0x12],
        reads: RefCell<usize>,
    }

    fn reply_21(id: u8, ack: bool, data: &[u8]) -> Vec<u8> {
        let mut b = vec![0u8; 49];
        b[0] = 0x21;
        b[13] = if ack { 0x80 } else { 0x00 };
        b[14] = id;
        b[15..15 + data.len()].copy_from_slice(data);
        b
    }

    impl FakeNs {
        fn new(respond: bool) -> Self {
            FakeNs {
                log: Rc::new(RefCell::new(Vec::new())),
                queue: RefCell::new(VecDeque::new()),
                respond,
                nack_first: RefCell::new(Vec::new()),
                silent: Vec::new(),
                user_spi: [0xFF; 0x16],
                factory_spi: factory_bytes(),
                reads: RefCell::new(0),
            }
        }

        fn writes(&self) -> Vec<Vec<u8>> {
            self.log
                .borrow()
                .iter()
                .filter_map(|e| match e {
                    Ev::Write(w) => Some(w.clone()),
                    _ => None,
                })
                .collect()
        }
    }

    impl HidIo for FakeNs {
        fn write(&self, data: &[u8]) -> Result<usize> {
            self.log.borrow_mut().push(Ev::Write(data.to_vec()));
            if !self.respond {
                return Ok(data.len());
            }
            let id = data[10];
            if self.silent.contains(&id) {
                return Ok(data.len());
            }
            let mut q = self.queue.borrow_mut();
            // Interleaved traffic the handshake must skip.
            let mut f30 = vec![0u8; 49];
            f30[0] = 0x30;
            q.push_back(f30);
            let mut f3f = vec![0u8; 12];
            f3f[0] = 0x3F;
            q.push_back(f3f);
            q.push_back(reply_21(0x40, true, &[]));
            {
                let mut nack = self.nack_first.borrow_mut();
                if let Some(pos) = nack.iter().position(|&n| n == id) {
                    nack.remove(pos);
                    q.push_back(reply_21(id, false, &[]));
                    return Ok(data.len());
                }
            }
            let data_out: Vec<u8> = if id == 0x10 {
                let addr = &data[11..15];
                let len = data[15];
                let spi: &[u8] = match u32::from_le_bytes([addr[0], addr[1], addr[2], addr[3]]) {
                    0x8010 => &self.user_spi,
                    0x603D => &self.factory_spi,
                    _ => &[],
                };
                let mut v = addr.to_vec();
                v.push(len);
                v.extend_from_slice(spi);
                v
            } else {
                Vec::new()
            };
            q.push_back(reply_21(id, true, &data_out));
            Ok(data.len())
        }

        fn read_timeout(&self, buf: &mut [u8], _ms: i32) -> Result<usize> {
            *self.reads.borrow_mut() += 1;
            match self.queue.borrow_mut().pop_front() {
                Some(f) => {
                    let n = f.len().min(buf.len());
                    buf[..n].copy_from_slice(&f[..n]);
                    Ok(n)
                }
                None => Ok(0),
            }
        }
    }

    fn pack(x: u16, y: u16) -> [u8; 3] {
        [(x & 0xFF) as u8, ((x >> 8) as u8 & 0x0F) | ((y & 0x0F) as u8) << 4, (y >> 4) as u8]
    }

    fn factory_bytes() -> [u8; 0x12] {
        let mut spi = [0u8; 0x12];
        spi[0..3].copy_from_slice(&pack(1500, 1600));
        spi[3..6].copy_from_slice(&pack(2010, 2020));
        spi[6..9].copy_from_slice(&pack(1300, 1400));
        spi[9..12].copy_from_slice(&pack(2030, 2040));
        spi[12..15].copy_from_slice(&pack(1100, 1200));
        spi[15..18].copy_from_slice(&pack(1450, 1550));
        spi
    }

    fn sleeper(log: &Rc<RefCell<Vec<Ev>>>) -> impl FnMut(Duration) {
        let log = log.clone();
        move |d| log.borrow_mut().push(Ev::Sleep(d))
    }

    /// Sum of injected sleeps between every pair of consecutive writes.
    fn gaps_between_writes(log: &[Ev]) -> Vec<Duration> {
        let mut gaps = Vec::new();
        let mut acc: Option<Duration> = None;
        for e in log {
            match e {
                Ev::Write(_) => {
                    if let Some(g) = acc {
                        gaps.push(g);
                    }
                    acc = Some(Duration::ZERO);
                }
                Ev::Sleep(d) => {
                    if let Some(g) = acc.as_mut() {
                        *g += *d;
                    }
                }
            }
        }
        gaps
    }

    #[test]
    fn ns_handshake_sends_cal_read_then_0x03_0x30_then_led() {
        let io = FakeNs::new(true);
        let mut sleep = sleeper(&io.log);
        let cal = ns_handshake(&io, &mut sleep);

        let writes = io.writes();
        let ids: Vec<u8> = writes.iter().map(|w| w[10]).collect();
        assert_eq!(ids, vec![0x10, 0x10, 0x03, 0x30, 0x48], "subcmd order");
        assert_eq!(&writes[0][11..16], &spi_read_args(0x8010, 0x16), "user cal read");
        assert_eq!(&writes[1][11..16], &spi_read_args(0x603D, 0x12), "factory cal read");
        assert_eq!(writes[2][11], 0x30, "0x03 arg = 0x30 (full report mode)");
        assert_eq!(writes[3][11], 0x01, "0x30 arg = player LED 1");
        assert!(writes.iter().all(|w| w.len() == 49 && w[0] == 0x01));
        let counters: Vec<u8> = writes.iter().map(|w| w[1]).collect();
        assert_eq!(counters, vec![0, 1, 2, 3, 4], "one counter per session");

        let gaps = gaps_between_writes(&io.log.borrow());
        assert_eq!(gaps.len(), 4);
        assert!(
            gaps.iter().all(|g| *g >= Duration::from_millis(60)),
            "≥ 60 ms between writes: {gaps:?}"
        );

        assert_eq!(cal, parse_factory_cal(&factory_bytes()));
        assert_ne!(cal, StickCal::default());
    }

    #[test]
    fn ns_handshake_prefers_user_cal() {
        let mut io = FakeNs::new(true);
        let mut user = [0u8; 0x16];
        user[0] = 0xB2;
        user[1] = 0xA1;
        user[2..5].copy_from_slice(&pack(1700, 1710));
        user[5..8].copy_from_slice(&pack(1990, 1995));
        user[8..11].copy_from_slice(&pack(1000, 1010));
        user[11] = 0xB2;
        user[12] = 0xA1;
        user[13..16].copy_from_slice(&pack(2001, 2002));
        user[16..19].copy_from_slice(&pack(1020, 1030));
        user[19..22].copy_from_slice(&pack(1040, 1050));
        io.user_spi = user;
        let mut sleep = sleeper(&io.log);
        let cal = ns_handshake(&io, &mut sleep);

        let ids: Vec<u8> = io.writes().iter().map(|w| w[10]).collect();
        assert_eq!(ids, vec![0x10, 0x03, 0x30, 0x48], "no factory read when user cal is valid");
        assert_eq!(cal, parse_user_cal(&user).unwrap());
    }

    #[test]
    fn ns_turn_off_sends_0x06_before_drop() {
        let io = FakeNs::new(false);
        let mut counter = 5u8;
        send_turn_off(&io, &mut counter);
        let writes = io.writes();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0][0], 0x01, "rumble+subcmd output report");
        assert_eq!(writes[0][1], 5, "session counter");
        assert_eq!(writes[0][10], 0x06, "subcmd id");
        assert_eq!(writes[0][11], 0x00, "arg");
        assert_eq!(counter, 6);
    }

    #[test]
    fn ns_handshake_counter_wraps_mod_16() {
        let io = FakeNs::new(true);
        let mut sleep = sleeper(&io.log);
        let mut counter = 14u8;
        ns_handshake_counted(&io, &mut sleep, &mut counter);
        let counters: Vec<u8> = io.writes().iter().map(|w| w[1]).collect();
        assert_eq!(counters, vec![14, 15, 0, 1, 2]);
        assert_eq!(counter, 3, "session counter continues after the handshake");
    }

    #[test]
    fn ns_handshake_retries_after_nack() {
        let io = FakeNs::new(true);
        io.nack_first.borrow_mut().push(0x03);
        let mut sleep = sleeper(&io.log);
        ns_handshake(&io, &mut sleep);
        let ids: Vec<u8> = io.writes().iter().map(|w| w[10]).collect();
        assert_eq!(ids, vec![0x10, 0x10, 0x03, 0x03, 0x30, 0x48]);
        let gaps = gaps_between_writes(&io.log.borrow());
        assert!(gaps.iter().all(|g| *g >= Duration::from_millis(60)), "{gaps:?}");
    }

    #[test]
    fn ns_handshake_no_ack_gives_up() {
        let io = FakeNs::new(false);
        let mut sleep = sleeper(&io.log);
        let t0 = Instant::now();
        let cal = ns_handshake(&io, &mut sleep);
        assert!(t0.elapsed() < Duration::from_secs(1), "must not block: {:?}", t0.elapsed());
        assert_eq!(cal, StickCal::default());

        let ids: Vec<u8> = io.writes().iter().map(|w| w[10]).collect();
        // 3 attempts each: user cal, factory cal, 0x03, 0x30, 0x48.
        assert_eq!(
            ids,
            vec![
                0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x03, 0x03, 0x03, 0x30, 0x30, 0x30, 0x48,
                0x48, 0x48,
            ]
        );
        let writes = io.writes();
        assert!(writes[..3].iter().all(|w| w[11..16] == spi_read_args(0x8010, 0x16)));
        assert!(writes[3..6].iter().all(|w| w[11..16] == spi_read_args(0x603D, 0x12)));
        // Each ACK wait is bounded by ~100 ms of short read timeouts.
        assert!(*io.reads.borrow() <= 15 * 20, "reads: {}", io.reads.borrow());
        let gaps = gaps_between_writes(&io.log.borrow());
        assert!(gaps.iter().all(|g| *g >= Duration::from_millis(60)), "{gaps:?}");
    }

    #[test]
    fn ns_reinit_sends_0x03_0x30_with_session_counter() {
        let io = FakeNs::new(false);
        let mut counter = 7u8;
        send_input_mode_full(&io, &mut counter);
        let w = io.writes();
        assert_eq!(w.len(), 1);
        assert_eq!((w[0][1], w[0][10], w[0][11]), (7, 0x03, 0x30));
        assert_eq!(counter, 8);
    }

    #[test]
    fn ns_handshake_enables_vibration_last() {
        let io = FakeNs::new(true);
        let mut sleep = sleeper(&io.log);
        let mut counter = 0u8;
        ns_handshake_counted(&io, &mut sleep, &mut counter);
        let writes = io.writes();
        let ids: Vec<u8> = writes.iter().map(|w| w[10]).collect();
        assert!(ids.ends_with(&[0x03, 0x30, 0x48]), "{ids:x?}");
        assert_eq!(writes.last().unwrap()[11], 0x01, "0x48 arg = enable");
    }

    #[test]
    fn ns_handshake_survives_missing_vibration_ack() {
        let mut io = FakeNs::new(true);
        io.silent = vec![0x48];
        let mut sleep = sleeper(&io.log);
        let cal = ns_handshake(&io, &mut sleep);
        let ids: Vec<u8> = io.writes().iter().map(|w| w[10]).collect();
        assert!(ids.ends_with(&[0x30, 0x48, 0x48, 0x48]), "3 attempts, then give up: {ids:x?}");
        assert_eq!(cal, parse_factory_cal(&factory_bytes()), "calibration still returned");
    }
}
