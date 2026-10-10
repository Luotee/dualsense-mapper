//! Pure Switch 2 Pro Controller (NS2 Pro, 057e:2069) BLE protocol
//! functions (no I/O, no config).
//!
//! Advertisement parsing, command framing, the 0x05 input report decoder,
//! SPI stick-calibration parsing and the `0x15` pairing crypto.
//!
//! NOTICE: the SPI stick-calibration block layout (factory blocks at
//! 0x13080 / 0x130C0, stick data at +0x28: neutral, rel_max, rel_min) and
//! the pairing address layout are adapted from BlueRetro
//! `main/bluetooth/hidp/sw2.{c,h}`, Copyright (c) 2025 Jacques Gagnon,
//! licensed under the Apache License, Version 2.0.

use aes::cipher::{generic_array::GenericArray, BlockEncrypt, KeyInit};

use super::ns_protocol::AxisCal;
use super::pad::PadState;

pub const NS2_COMPANY_ID: u16 = 0x0553;
pub const NS2_VID: u16 = 0x057E;
pub const NS2_PRO_PID: u16 = 0x2069;

/// Command: SPI flash read (sub-command 0x04).
pub const CMD_SPI: u8 = 0x02;
pub const SUB_SPI_READ: u8 = 0x04;
/// Command: player LED (sub-command 0x07).
pub const CMD_LED: u8 = 0x09;
pub const SUB_LED_SET: u8 = 0x07;
/// Command: controller power (sub-command 0x02 = turn off).
pub const CMD_POWER: u8 = 0x06;
pub const SUB_POWER_OFF: u8 = 0x02;
/// Command: pairing. Steps in order: 0x01 addresses, 0x04 A1, 0x02 A2, 0x03 commit.
pub const CMD_PAIR: u8 = 0x15;
pub const SUB_PAIR_ADDR: u8 = 0x01;
pub const SUB_PAIR_A1: u8 = 0x04;
pub const SUB_PAIR_A2: u8 = 0x02;
pub const SUB_PAIR_COMMIT: u8 = 0x03;

/// SPI factory stick-calibration blocks (read `SPI_CAL_LEN` bytes each).
pub const SPI_LEFT_STICK_CAL: u32 = 0x13080;
pub const SPI_RIGHT_STICK_CAL: u32 = 0x130C0;
pub const SPI_CAL_LEN: u8 = 0x40;

const REQ_TYPE: u8 = 0x91;
const RSP_TYPE: u8 = 0x01;
const IFACE_BLE: u8 = 0x01;
const HEADER_LEN: usize = 8;
/// SPI read response: 8-byte header, then `len, 0, 0, 0, addr LE u32`.
const SPI_RSP_DATA: usize = HEADER_LEN + 8;
/// Stick record offset inside a factory calibration block.
const STICK_REC: usize = 0x28;
const STICK_REC_LEN: usize = 9;
const DEFAULT_CENTER: u16 = 2048;
const DEFAULT_SPAN: u16 = 1400;
const UNINIT: u16 = 0xFFF;

/// Manufacturer-data fields of an NS2 advertisement (company id 0x0553).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ns2Advert {
    pub pid: u16,
    /// Host the pad is bonded to, as advertised (LSB-first); all zero
    /// while SYNC is held.
    pub host: [u8; 6],
}

/// Parse manufacturer data *after* the 2-byte company id (btleplug strips
/// it): `[3..5]` VID LE, `[5..7]` PID LE, `[10..16]` host address.
/// `None` unless the VID is Nintendo's.
pub fn parse_advert(mfr_data_after_company_id: &[u8]) -> Option<Ns2Advert> {
    let d = mfr_data_after_company_id;
    if d.len() < 16 || u16::from_le_bytes([d[3], d[4]]) != NS2_VID {
        return None;
    }
    let mut host = [0u8; 6];
    host.copy_from_slice(&d[10..16]);
    Some(Ns2Advert { pid: u16::from_le_bytes([d[5], d[6]]), host })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdvertMatch {
    /// Bonded to this adapter: connect, no pairing needed.
    Bonded,
    /// SYNC held: connect and run the 0x15 pairing.
    PairingMode,
    /// Not an NS2 Pro, or bonded to another host.
    Other,
}

/// Classify an advert for our adapter. The host field is our address
/// LSB-first when bonded to us (hardware-proven), all zero in pairing
/// mode. Pads other than the NS2 Pro are `Other`.
pub fn classify_advert(ad: &Ns2Advert, local_addr_msb_first: [u8; 6]) -> AdvertMatch {
    if ad.pid != NS2_PRO_PID {
        return AdvertMatch::Other;
    }
    if ad.host == [0u8; 6] {
        return AdvertMatch::PairingMode;
    }
    let mut ours = local_addr_msb_first;
    ours.reverse();
    if ad.host == ours {
        AdvertMatch::Bonded
    } else {
        AdvertMatch::Other
    }
}

/// Frame a command: `cmd, 0x91, 0x01, sub, 0x00, len, 0x00, 0x00` + data.
/// Data beyond 255 bytes is truncated (no command needs more).
pub fn command(cmd: u8, sub: u8, data: &[u8]) -> Vec<u8> {
    let data = &data[..data.len().min(u8::MAX as usize)];
    let mut out = Vec::with_capacity(HEADER_LEN + data.len());
    out.extend_from_slice(&[cmd, REQ_TYPE, IFACE_BLE, sub, 0x00, data.len() as u8, 0x00, 0x00]);
    out.extend_from_slice(data);
    out
}

/// True when `buf` is a full response header for `cmd`/`sub`
/// (`cmd, 0x01, _, sub, …`).
pub fn is_response_to(buf: &[u8], cmd: u8, sub: u8) -> bool {
    buf.len() >= HEADER_LEN && buf[0] == cmd && buf[1] == RSP_TYPE && buf[3] == sub
}

/// SPI flash read: data `[len, 0x7e, 0, 0, addr LE u32]`.
pub fn spi_read_cmd(addr: u32, len: u8) -> Vec<u8> {
    let a = addr.to_le_bytes();
    command(CMD_SPI, SUB_SPI_READ, &[len, 0x7e, 0x00, 0x00, a[0], a[1], a[2], a[3]])
}

/// Data bytes of an SPI read response, `None` unless the response is an
/// SPI read reply carrying all `len` bytes it announces.
pub fn spi_read_payload(resp: &[u8]) -> Option<&[u8]> {
    if !is_response_to(resp, CMD_SPI, SUB_SPI_READ) || resp.len() < SPI_RSP_DATA {
        return None;
    }
    let n = resp[HEADER_LEN] as usize;
    resp.get(SPI_RSP_DATA..SPI_RSP_DATA + n)
}

/// Per-axis stick calibration (raw 12-bit units).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ns2StickCal {
    pub lx: AxisCal,
    pub ly: AxisCal,
    pub rx: AxisCal,
    pub ry: AxisCal,
}

impl Default for Ns2StickCal {
    fn default() -> Self {
        let d = AxisCal { center: DEFAULT_CENTER, span_neg: DEFAULT_SPAN, span_pos: DEFAULT_SPAN };
        Ns2StickCal { lx: d, ly: d, rx: d, ry: d }
    }
}

/// Unpack two 12-bit values from 3 bytes (x low, y high).
fn unpack12(b: &[u8]) -> (u16, u16) {
    let x = (b[0] as u16) | (((b[1] & 0x0F) as u16) << 8);
    let y = ((b[1] >> 4) as u16) | ((b[2] as u16) << 4);
    (x, y)
}

fn axis(center: u16, neg: u16, pos: u16) -> AxisCal {
    let valid = |v: u16| v != 0 && v != UNINIT;
    AxisCal {
        center: if valid(center) { center } else { DEFAULT_CENTER },
        span_neg: if valid(neg) { neg } else { DEFAULT_SPAN },
        span_pos: if valid(pos) { pos } else { DEFAULT_SPAN },
    }
}

/// One stick from a factory block: record at +0x28 = neutral, rel_max,
/// rel_min (12-bit x/y pairs; rel_* are spans from neutral). Erased flash
/// (first record byte 0xFF, as BlueRetro checks) or a short block → `None`.
fn parse_stick(block: &[u8]) -> Option<(AxisCal, AxisCal)> {
    let rec = block.get(STICK_REC..STICK_REC + STICK_REC_LEN)?;
    if rec[0] == 0xFF {
        return None;
    }
    let (xc, yc) = unpack12(&rec[0..3]);
    let (xmax, ymax) = unpack12(&rec[3..6]);
    let (xmin, ymin) = unpack12(&rec[6..9]);
    Some((axis(xc, xmin, xmax), axis(yc, ymin, ymax)))
}

/// Parse the factory calibration blocks read from `SPI_LEFT_STICK_CAL` /
/// `SPI_RIGHT_STICK_CAL` (`SPI_CAL_LEN` bytes each, as returned by
/// `spi_read_payload`). An invalid stick keeps the defaults.
pub fn parse_stick_cal(left: &[u8], right: &[u8]) -> Ns2StickCal {
    let mut cal = Ns2StickCal::default();
    if let Some((x, y)) = parse_stick(left) {
        cal.lx = x;
        cal.ly = y;
    }
    if let Some((x, y)) = parse_stick(right) {
        cal.rx = x;
        cal.ry = y;
    }
    cal
}

fn norm(raw: u16, c: &AxisCal) -> f32 {
    let d = raw as f32 - c.center as f32;
    let v = if d >= 0.0 { d / c.span_pos.max(1) as f32 } else { d / c.span_neg.max(1) as f32 };
    v.clamp(-1.0, 1.0)
}

/// Decode an input report 0x05 (63 bytes on the wire, no report id).
/// `None` if shorter than the 16 bytes that carry buttons and sticks.
pub fn decode_05(buf: &[u8], cal: &Ns2StickCal) -> Option<PadState> {
    if buf.len() < 16 {
        return None;
    }
    let mut s = PadState::default();
    let (b4, b5, b6, b7) = (buf[4], buf[5], buf[6], buf[7]);

    s.buttons[2] = b4 & 0x01 != 0; // Y
    s.buttons[3] = b4 & 0x02 != 0; // X
    s.buttons[0] = b4 & 0x04 != 0; // B
    s.buttons[1] = b4 & 0x08 != 0; // A
    s.buttons[10] = b4 & 0x40 != 0; // R
    s.buttons[24] = b4 & 0x80 != 0; // ZR

    s.buttons[4] = b5 & 0x01 != 0; // Minus
    s.buttons[6] = b5 & 0x02 != 0; // Plus
    s.buttons[8] = b5 & 0x04 != 0; // RS
    s.buttons[7] = b5 & 0x08 != 0; // LS
    s.buttons[5] = b5 & 0x10 != 0; // Home
    s.buttons[29] = b5 & 0x20 != 0; // Capture
    s.buttons[32] = b5 & 0x40 != 0; // C

    s.buttons[12] = b6 & 0x01 != 0; // Down
    s.buttons[11] = b6 & 0x02 != 0; // Up
    s.buttons[14] = b6 & 0x04 != 0; // Right
    s.buttons[13] = b6 & 0x08 != 0; // Left
    s.buttons[9] = b6 & 0x40 != 0; // L
    s.buttons[23] = b6 & 0x80 != 0; // ZL

    s.buttons[31] = b7 & 0x01 != 0; // GR
    s.buttons[30] = b7 & 0x02 != 0; // GL

    s.trigger_l2 = if s.buttons[23] { 1.0 } else { 0.0 };
    s.trigger_r2 = if s.buttons[24] { 1.0 } else { 0.0 };

    let (lx, ly) = unpack12(&buf[10..13]);
    let (rx, ry) = unpack12(&buf[13..16]);
    s.stick_lx = norm(lx, &cal.lx);
    s.stick_ly = norm(ly, &cal.ly);
    s.stick_rx = norm(rx, &cal.rx);
    s.stick_ry = norm(ry, &cal.ry);
    Some(s)
}

/// Host-chosen pairing nonces A1 (step 0x04) and A2 (step 0x02).
pub struct PairingKeys {
    pub a1: [u8; 16],
    pub a2: [u8; 16],
}

impl PairingKeys {
    pub fn random() -> Self {
        PairingKeys { a1: rand::random(), a2: rand::random() }
    }
}

/// Step 0x01 data: `0x00, 0x02`, host LSB-first, (host − 1) LSB-first.
/// Like BlueRetro, "− 1" decrements only the least-significant byte
/// (wrapping, no borrow).
pub fn pairing_addresses(local_msb_first: [u8; 6]) -> [u8; 14] {
    let mut lsb = local_msb_first;
    lsb.reverse();
    let mut out = [0u8; 14];
    out[0] = 0x00;
    out[1] = 0x02;
    out[2..8].copy_from_slice(&lsb);
    out[8..14].copy_from_slice(&lsb);
    out[8] = lsb[0].wrapping_sub(1);
    out
}

/// Pairing step carrying a 16-byte nonce (A1 on 0x04, A2 on 0x02):
/// data `0x00` + key.
pub fn pairing_key_cmd(sub: u8, key: &[u8; 16]) -> Vec<u8> {
    let mut data = [0u8; 17];
    data[1..].copy_from_slice(key);
    command(CMD_PAIR, sub, &data)
}

/// The 16-byte key (B1 / B2) in a pairing response: header, one status
/// byte, then the key.
pub fn pairing_response_key(resp: &[u8], sub: u8) -> Option<[u8; 16]> {
    if !is_response_to(resp, CMD_PAIR, sub) {
        return None;
    }
    resp.get(HEADER_LEN + 1..HEADER_LEN + 17)?.try_into().ok()
}

/// Long-term key = A1 XOR B1.
pub fn ltk(a1: &[u8; 16], b1: &[u8; 16]) -> [u8; 16] {
    let mut k = [0u8; 16];
    for i in 0..16 {
        k[i] = a1[i] ^ b1[i];
    }
    k
}

/// The B2 the pad must answer to A2: AES-128-ECB with key = reversed LTK
/// over the reversed A2; the result is compared to B2 as received.
pub fn expected_b2(ltk: &[u8; 16], a2: &[u8; 16]) -> [u8; 16] {
    let mut key = *ltk;
    key.reverse();
    let mut block = *a2;
    block.reverse();
    let cipher = aes::Aes128::new(GenericArray::from_slice(&key));
    let mut b = GenericArray::clone_from_slice(&block);
    cipher.encrypt_block(&mut b);
    b.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse a space-separated hex dump as printed in the spike logs.
    fn hex(s: &str) -> Vec<u8> {
        s.split_whitespace().map(|b| u8::from_str_radix(b, 16).unwrap()).collect()
    }

    fn key(s: &str) -> [u8; 16] {
        hex(s).try_into().unwrap()
    }

    fn pack(x: u16, y: u16) -> [u8; 3] {
        [(x & 0xFF) as u8, ((x >> 8) as u8 & 0x0F) | ((y & 0x0F) as u8) << 4, (y >> 4) as u8]
    }

    fn report(b: [u8; 4], l: (u16, u16), r: (u16, u16)) -> [u8; 63] {
        let mut buf = [0u8; 63];
        buf[4..8].copy_from_slice(&b);
        buf[10..13].copy_from_slice(&pack(l.0, l.1));
        buf[13..16].copy_from_slice(&pack(r.0, r.1));
        buf
    }

    fn neutral(b: [u8; 4]) -> [u8; 63] {
        report(b, (2048, 2048), (2048, 2048))
    }

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-6
    }

    // ── Hardware vectors (spike logs, local adapter 34:FD:70:A3:21:85) ──

    const LOCAL: [u8; 6] = [0x34, 0xFD, 0x70, 0xA3, 0x21, 0x85];

    /// Round-2 `first report (63 bytes)`: pad at rest, nothing pressed.
    const R2_FIRST_REPORT: &str = "69 0d 00 00 00 00 00 00 00 00 ef 27 86 9d 88 7d 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 1d 0e 00 00 00 00 00 00 00 00 01 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00";

    /// Round-1 `ADV … raw=` with SYNC held (host field zero).
    const ADV_PAIRING: &str = "01 00 03 7e 05 69 20 00 01 00 00 00 00 00 00 00 0f 00 00 00 00 00 00 00";
    /// Round-1 (after pairing) / round-2 `ADV … raw=` woken by a button.
    const ADV_BONDED: &str = "01 00 03 7e 05 69 20 00 01 00 85 21 a3 70 fd 34 0f 00 00 00 00 00 00 00";

    #[test]
    fn real_report_decodes() {
        let buf = hex(R2_FIRST_REPORT);
        assert_eq!(buf.len(), 63);
        let s = decode_05(&buf, &Ns2StickCal::default()).unwrap();
        assert!(s.buttons.iter().all(|&p| !p), "nothing pressed");
        assert_eq!((s.trigger_l2, s.trigger_r2), (0.0, 0.0));
        // Raw 12-bit values: L=(2031,2146) R=(2205,2008).
        assert!(approx(s.stick_lx, (2031.0 - 2048.0) / 1400.0), "lx {}", s.stick_lx);
        assert!(approx(s.stick_ly, (2146.0 - 2048.0) / 1400.0), "ly {}", s.stick_ly);
        assert!(approx(s.stick_rx, (2205.0 - 2048.0) / 1400.0), "rx {}", s.stick_rx);
        assert!(approx(s.stick_ry, (2008.0 - 2048.0) / 1400.0), "ry {}", s.stick_ry);
        assert!(!s.finger0_active && !s.touchpad_btn);
        // With a calibration centred on this pad's rest point, rest is 0.
        let mut cal = Ns2StickCal::default();
        cal.lx.center = 2031;
        cal.ly.center = 2146;
        cal.rx.center = 2205;
        cal.ry.center = 2008;
        let s = decode_05(&buf, &cal).unwrap();
        assert_eq!((s.stick_lx, s.stick_ly, s.stick_rx, s.stick_ry), (0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn decode_rejects_short() {
        assert!(decode_05(&[0u8; 15], &Ns2StickCal::default()).is_none());
        assert!(decode_05(&[0u8; 16], &Ns2StickCal::default()).is_some());
    }

    #[test]
    fn each_button_bit() {
        // (report byte, mask, button id)
        let cases: &[(usize, u8, usize)] = &[
            (4, 0x01, 2), (4, 0x02, 3), (4, 0x04, 0), (4, 0x08, 1), (4, 0x40, 10), (4, 0x80, 24),
            (5, 0x01, 4), (5, 0x02, 6), (5, 0x04, 8), (5, 0x08, 7), (5, 0x10, 5), (5, 0x20, 29),
            (5, 0x40, 32),
            (6, 0x01, 12), (6, 0x02, 11), (6, 0x04, 14), (6, 0x08, 13), (6, 0x40, 9), (6, 0x80, 23),
            (7, 0x01, 31), (7, 0x02, 30),
        ];
        for &(byte, mask, id) in cases {
            let mut b = neutral([0; 4]);
            b[byte] = mask;
            let s = decode_05(&b, &Ns2StickCal::default()).unwrap();
            for (i, &p) in s.buttons.iter().enumerate() {
                assert_eq!(p, i == id, "byte {byte} mask {mask:#04x}: slot {i}");
            }
        }
    }

    #[test]
    fn unmapped_bits_set_nothing() {
        // Every bit not in the table, plus the counter bytes: no button,
        // never the DualSense-only ids 15..=22, no touch fields.
        let mut b = neutral([0x30, 0x80, 0x30, 0xFC]);
        b[0..4].copy_from_slice(&[0xFF; 4]);
        b[8] = 0xFF;
        b[9] = 0xFF;
        let s = decode_05(&b, &Ns2StickCal::default()).unwrap();
        assert!(s.buttons.iter().all(|&p| !p));
        assert!(!s.finger0_active && !s.touchpad_btn);
        assert_eq!((s.finger0_x, s.finger0_y), (0, 0));
    }

    #[test]
    fn zl_zr_trigger() {
        let s = decode_05(&neutral([0x80, 0, 0x80, 0]), &Ns2StickCal::default()).unwrap();
        assert!(s.buttons[23] && s.buttons[24]);
        assert_eq!((s.trigger_l2, s.trigger_r2), (1.0, 1.0));
        let s = decode_05(&neutral([0, 0, 0x80, 0]), &Ns2StickCal::default()).unwrap();
        assert_eq!((s.trigger_l2, s.trigger_r2), (1.0, 0.0));
        let s = decode_05(&neutral([0x80, 0, 0, 0]), &Ns2StickCal::default()).unwrap();
        assert_eq!((s.trigger_l2, s.trigger_r2), (0.0, 1.0));
        let s = decode_05(&neutral([0; 4]), &Ns2StickCal::default()).unwrap();
        assert_eq!((s.trigger_l2, s.trigger_r2), (0.0, 0.0));
    }

    #[test]
    fn stick_center_zero() {
        let s = decode_05(&neutral([0; 4]), &Ns2StickCal::default()).unwrap();
        assert_eq!((s.stick_lx, s.stick_ly, s.stick_rx, s.stick_ry), (0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn stick_y_up_positive_and_clamped() {
        let b = report([0; 4], (2048, 2048 + 700), (2048, 2048 - 700));
        let s = decode_05(&b, &Ns2StickCal::default()).unwrap();
        assert!(approx(s.stick_ly, 0.5));
        assert!(approx(s.stick_ry, -0.5));
        let b = report([0; 4], (0, 4095), (4095, 0));
        let s = decode_05(&b, &Ns2StickCal::default()).unwrap();
        assert_eq!((s.stick_lx, s.stick_ly, s.stick_rx, s.stick_ry), (-1.0, 1.0, 1.0, -1.0));
    }

    #[test]
    fn stick_uses_asymmetric_spans() {
        let mut cal = Ns2StickCal::default();
        cal.lx = AxisCal { center: 2000, span_neg: 1000, span_pos: 2000 };
        let s = decode_05(&report([0; 4], (3000, 2048), (2048, 2048)), &cal).unwrap();
        assert!(approx(s.stick_lx, 0.5));
        let s = decode_05(&report([0; 4], (1500, 2048), (2048, 2048)), &cal).unwrap();
        assert!(approx(s.stick_lx, -0.5));
        cal.lx = AxisCal { center: 2048, span_neg: 0, span_pos: 0 };
        let s = decode_05(&report([0; 4], (4095, 2048), (2048, 2048)), &cal).unwrap();
        assert!(s.stick_lx.is_finite());
    }

    #[test]
    fn default_cal_is_2048_1400() {
        let d = AxisCal { center: 2048, span_neg: 1400, span_pos: 1400 };
        assert_eq!(Ns2StickCal::default(), Ns2StickCal { lx: d, ly: d, rx: d, ry: d });
    }

    /// Build a 0x40-byte factory block with the stick record at +0x28.
    fn cal_block(neutral: (u16, u16), rel_max: (u16, u16), rel_min: (u16, u16)) -> Vec<u8> {
        let mut b = vec![0xA5u8; 0x40];
        b[0x28..0x2B].copy_from_slice(&pack(neutral.0, neutral.1));
        b[0x2B..0x2E].copy_from_slice(&pack(rel_max.0, rel_max.1));
        b[0x2E..0x31].copy_from_slice(&pack(rel_min.0, rel_min.1));
        b
    }

    #[test]
    fn stick_cal_parses_factory_blocks() {
        let left = cal_block((1971, 2102), (1582, 1510), (1594, 1520));
        let right = cal_block((2060, 1990), (1600, 1550), (1580, 1530));
        let c = parse_stick_cal(&left, &right);
        assert_eq!(c.lx, AxisCal { center: 1971, span_neg: 1594, span_pos: 1582 });
        assert_eq!(c.ly, AxisCal { center: 2102, span_neg: 1520, span_pos: 1510 });
        assert_eq!(c.rx, AxisCal { center: 2060, span_neg: 1580, span_pos: 1600 });
        assert_eq!(c.ry, AxisCal { center: 1990, span_neg: 1530, span_pos: 1550 });
    }

    #[test]
    fn stick_cal_invalid_falls_back() {
        let good = cal_block((1971, 2102), (1582, 1510), (1594, 1520));
        let d = Ns2StickCal::default();
        // Erased flash.
        assert_eq!(parse_stick_cal(&[0xFF; 0x40], &[0xFF; 0x40]), d);
        // Too short to reach the record.
        assert_eq!(parse_stick_cal(&good[..0x30], &good[..0x30]), d);
        assert_eq!(parse_stick_cal(&[], &[]), d);
        // One bad stick keeps the other stick's calibration.
        let c = parse_stick_cal(&good, &[0xFF; 0x40]);
        assert_eq!((c.rx, c.ry), (d.rx, d.ry));
        assert_eq!(c.lx.center, 1971);
        // Zero / 0xFFF fields fall back per axis.
        let odd = cal_block((0, 2102), (0xFFF, 1510), (1594, 0));
        let c = parse_stick_cal(&odd, &odd);
        assert_eq!(c.lx.center, 2048);
        assert_eq!(c.lx.span_pos, 1400);
        assert_eq!(c.ly.span_neg, 1400);
        assert_eq!(c.ly.center, 2102);
    }

    #[test]
    fn command_framing_matches_hardware() {
        assert_eq!(
            command(0x09, 0x07, &[0x01, 0, 0, 0, 0, 0, 0, 0]),
            hex("09 91 01 07 00 08 00 00 01 00 00 00 00 00 00 00")
        );
        let mut off = hex("06 91 01 02 00 0c 00 00");
        off.extend_from_slice(&[0; 12]);
        assert_eq!(command(0x06, 0x02, &[0; 12]), off);
        assert_eq!(command(0x15, 0x03, &[0x00]), hex("15 91 01 03 00 01 00 00 00"));
    }

    #[test]
    fn response_matching() {
        let led = hex("09 01 01 07 10 78 00 00");
        assert!(is_response_to(&led, 0x09, 0x07));
        assert!(!is_response_to(&led, 0x09, 0x06), "wrong sub");
        assert!(!is_response_to(&led, 0x02, 0x07), "wrong cmd");
        assert!(!is_response_to(&hex("09 91 01 07 00 08 00 00"), 0x09, 0x07), "a request echo");
        assert!(!is_response_to(&led[..4], 0x09, 0x07), "truncated header");
    }

    #[test]
    fn spi_read_matches_hardware() {
        assert_eq!(spi_read_cmd(0x13000, 0x10), hex("02 91 01 04 00 08 00 00 10 7e 00 00 00 30 01 00"));
        assert_eq!(
            spi_read_cmd(0x130C0, 0x40),
            hex("02 91 01 04 00 08 00 00 40 7e 00 00 c0 30 01 00")
        );
        let rx = hex("02 01 01 04 10 78 00 00 10 00 00 00 00 30 01 00 01 00 48 45 4b 31 30 30 30 30 33 38 30 39 34 31");
        let data = spi_read_payload(&rx).unwrap();
        assert_eq!(data.len(), 0x10);
        assert_eq!(&data[2..], b"HEK10000380941" as &[u8]);
        assert_eq!(&data[..2], &[0x01, 0x00]);
        // Truncated payload, wrong command, short header.
        assert!(spi_read_payload(&rx[..rx.len() - 1]).is_none());
        assert!(spi_read_payload(&hex("09 01 01 07 10 78 00 00")).is_none());
        assert!(spi_read_payload(&rx[..12]).is_none());
    }

    #[test]
    fn advert_classification() {
        let p = parse_advert(&hex(ADV_PAIRING)).unwrap();
        assert_eq!(p, Ns2Advert { pid: NS2_PRO_PID, host: [0; 6] });
        assert_eq!(classify_advert(&p, LOCAL), AdvertMatch::PairingMode);

        let b = parse_advert(&hex(ADV_BONDED)).unwrap();
        assert_eq!(b.host, [0x85, 0x21, 0xa3, 0x70, 0xfd, 0x34]);
        assert_eq!(classify_advert(&b, LOCAL), AdvertMatch::Bonded);

        // Bonded to another host; host field equal to our address in
        // MSB-first order is NOT ours (hardware proved LSB-first).
        let mut other = LOCAL;
        other[5] = 0x86;
        assert_eq!(classify_advert(&b, other), AdvertMatch::Other);
        let msb = Ns2Advert { pid: NS2_PRO_PID, host: LOCAL };
        assert_eq!(classify_advert(&msb, LOCAL), AdvertMatch::Other);

        // Another Nintendo pad (Joy-Con 2 R) in pairing mode is not ours.
        let jc = Ns2Advert { pid: 0x2067, host: [0; 6] };
        assert_eq!(classify_advert(&jc, LOCAL), AdvertMatch::Other);
    }

    #[test]
    fn advert_parse_rejects() {
        let raw = hex(ADV_BONDED);
        assert!(parse_advert(&raw[..15]).is_none(), "too short for host field");
        assert!(parse_advert(&raw[..16]).is_some());
        let mut wrong_vid = raw.clone();
        wrong_vid[3] = 0x4c;
        assert!(parse_advert(&wrong_vid).is_none());
    }

    #[test]
    fn pairing_addresses_layout() {
        let a = pairing_addresses(LOCAL);
        assert_eq!(a.to_vec(), hex("00 02 85 21 a3 70 fd 34 84 21 a3 70 fd 34"));
        assert_eq!(
            command(0x15, 0x01, &a),
            hex("15 91 01 01 00 0e 00 00 00 02 85 21 a3 70 fd 34 84 21 a3 70 fd 34")
        );
        // Like BlueRetro: only the least-significant byte is decremented
        // (wrapping, no borrow).
        let a = pairing_addresses([0x11, 0x22, 0x33, 0x44, 0x55, 0x00]);
        assert_eq!(&a[2..8], &[0x00, 0x55, 0x44, 0x33, 0x22, 0x11]);
        assert_eq!(&a[8..14], &[0xFF, 0x55, 0x44, 0x33, 0x22, 0x11]);
    }

    #[test]
    fn pairing_vector_from_hardware() {
        let a1 = key("6d 3e 5d 34 1e 42 af dd 0d 1f e0 e9 b9 e6 8a 0b");
        let a2 = key("8e 2a 09 a6 1e ff ef c7 21 94 65 66 ce 5c 0a c5");

        // Our frames are byte-identical to what the pad accepted.
        assert_eq!(
            pairing_key_cmd(0x04, &a1),
            hex("15 91 01 04 00 11 00 00 00 6d 3e 5d 34 1e 42 af dd 0d 1f e0 e9 b9 e6 8a 0b")
        );
        assert_eq!(
            pairing_key_cmd(0x02, &a2),
            hex("15 91 01 02 00 11 00 00 00 8e 2a 09 a6 1e ff ef c7 21 94 65 66 ce 5c 0a c5")
        );

        // B1 and B2 extracted from the pad's real replies.
        let rx04 = hex("15 01 01 04 10 78 00 00 01 5c f6 ee 79 2c df 05 e1 ba 2b 63 25 c4 1a 5f 10");
        let rx02 = hex("15 01 01 02 10 78 00 00 01 05 02 20 45 93 c6 bb a6 dc d3 1c fc 37 c6 3a 73");
        let b1 = pairing_response_key(&rx04, 0x04).unwrap();
        assert_eq!(b1, key("5c f6 ee 79 2c df 05 e1 ba 2b 63 25 c4 1a 5f 10"));
        let b2 = pairing_response_key(&rx02, 0x02).unwrap();
        assert_eq!(b2, key("05 02 20 45 93 c6 bb a6 dc d3 1c fc 37 c6 3a 73"));
        assert!(pairing_response_key(&rx02, 0x04).is_none(), "sub mismatch");
        assert!(pairing_response_key(&rx02[..24], 0x02).is_none(), "truncated");

        // The load-bearing check: our crypto reproduces the pad's B2.
        let k = ltk(&a1, &b1);
        assert_eq!(expected_b2(&k, &a2), b2);
    }

    #[test]
    fn ltk_is_xor() {
        let a = [0xF0u8; 16];
        let mut b = [0u8; 16];
        b[0] = 0x0F;
        b[15] = 0xF0;
        let k = ltk(&a, &b);
        assert_eq!(k[0], 0xFF);
        assert_eq!(k[1], 0xF0);
        assert_eq!(k[15], 0x00);
    }

    #[test]
    fn random_pairing_keys_differ() {
        let k1 = PairingKeys::random();
        let k2 = PairingKeys::random();
        assert_ne!(k1.a1, k1.a2);
        assert_ne!(k1.a1, k2.a1);
    }
}
