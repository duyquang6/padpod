// Copyright 2026 ligt (https://github.com/duyquang6/padpod)
// SPDX-License-Identifier: GPL-3.0-only

//! DualShock 4 (PS4 controller), as it speaks over Bluetooth.
//!
//! Hosts that only take controllers they know - iPadOS above all, and games
//! there such as Genshin - recognise this one by its IDs (Sony, 054C:09CC)
//! and its reports, so in this mode the handheld answers to both:
//!
//! - **Input report 0x11**, 78 bytes after the DATA header: the USB report's
//!   fields behind two flag bytes, with a CRC-32 over the header and report at
//!   the end. Layout as `struct dualshock4_input_report_bt` in Linux's
//!   `hid-playstation.c`.
//! - **Feature reports** the host reads on connecting: calibration (0x05 on
//!   Bluetooth, with its own CRC; 0x02 as over USB), firmware info (0xA3) and
//!   pairing info (0x12). Linux refuses the controller's motion sensors, not
//!   the controller, when calibration is missing - but other hosts are less
//!   forgiving, so all of them are answered with plausible values.
//!
//! The descriptor is the Bluetooth one Linux's `hid-sony.c` (v4.9) carried for
//! this controller.

use crate::hid::{self, button, State};
use crate::l2cap::Addr;

pub const VENDOR: u16 = 0x054C;
pub const PRODUCT: u16 = 0x09CC;
/// What a real one calls itself.
pub const NAME: &str = "Wireless Controller";

pub const DESCRIPTOR: &[u8] = &[
    0x05, 0x01,       // Usage Page (Desktop)
    0x09, 0x05,       // Usage (Gamepad)
    0xA1, 0x01,       // Collection (Application)
    0x85, 0x01,       // Report ID (1)
    0x75, 0x08,       // Report Size (8)
    0x95, 0x0A,       // Report Count (10)
    0x81, 0x02,       // Input (Variable)
    0x06, 0x04, 0xFF, // Usage Page (FF04h)
    0x85, 0x02,       // Report ID (2)
    0x09, 0x24,       // Usage (24h)
    0x95, 0x24,       // Report Count (36)
    0xB1, 0x02,       // Feature (Variable)
    0x85, 0xA3,       // Report ID (163)
    0x09, 0x25,       // Usage (25h)
    0x95, 0x30,       // Report Count (48)
    0xB1, 0x02,       // Feature (Variable)
    0x85, 0x05,       // Report ID (5)
    0x09, 0x26,       // Usage (26h)
    0x95, 0x28,       // Report Count (40)
    0xB1, 0x02,       // Feature (Variable)
    0x85, 0x06,       // Report ID (6)
    0x09, 0x27,       // Usage (27h)
    0x95, 0x34,       // Report Count (52)
    0xB1, 0x02,       // Feature (Variable)
    0x85, 0x07,       // Report ID (7)
    0x09, 0x28,       // Usage (28h)
    0x95, 0x30,       // Report Count (48)
    0xB1, 0x02,       // Feature (Variable)
    0x85, 0x08,       // Report ID (8)
    0x09, 0x29,       // Usage (29h)
    0x95, 0x2F,       // Report Count (47)
    0xB1, 0x02,       // Feature (Variable)
    0x06, 0x03, 0xFF, // Usage Page (FF03h)
    0x85, 0x03,       // Report ID (3)
    0x09, 0x21,       // Usage (21h)
    0x95, 0x26,       // Report Count (38)
    0xB1, 0x02,       // Feature (Variable)
    0x85, 0x04,       // Report ID (4)
    0x09, 0x22,       // Usage (22h)
    0x95, 0x2E,       // Report Count (46)
    0xB1, 0x02,       // Feature (Variable)
    0x85, 0xF0,       // Report ID (240)
    0x09, 0x47,       // Usage (47h)
    0x95, 0x3F,       // Report Count (63)
    0xB1, 0x02,       // Feature (Variable)
    0x85, 0xF1,       // Report ID (241)
    0x09, 0x48,       // Usage (48h)
    0x95, 0x3F,       // Report Count (63)
    0xB1, 0x02,       // Feature (Variable)
    0x85, 0xF2,       // Report ID (242)
    0x09, 0x49,       // Usage (49h)
    0x95, 0x0F,       // Report Count (15)
    0xB1, 0x02,       // Feature (Variable)
    0x85, 0x11,       // Report ID (17)
    0x06, 0x00, 0xFF, // Usage Page (FF00h)
    0x09, 0x20,       // Usage (20h)
    0x95, 0x02,       // Report Count (2)
    0x81, 0x02,       // Input (Variable)
    0x05, 0x01,       // Usage Page (Desktop)
    0x09, 0x30,       // Usage (X)
    0x09, 0x31,       // Usage (Y)
    0x09, 0x32,       // Usage (Z)
    0x09, 0x35,       // Usage (Rz)
    0x15, 0x00,       // Logical Minimum (0)
    0x26, 0xFF, 0x00, // Logical Maximum (255)
    0x75, 0x08,       // Report Size (8)
    0x95, 0x04,       // Report Count (4)
    0x81, 0x02,       // Input (Variable)
    0x09, 0x39,       // Usage (Hat Switch)
    0x15, 0x00,       // Logical Minimum (0)
    0x25, 0x07,       // Logical Maximum (7)
    0x75, 0x04,       // Report Size (4)
    0x95, 0x01,       // Report Count (1)
    0x81, 0x42,       // Input (Variable, Null State)
    0x05, 0x09,       // Usage Page (Button)
    0x19, 0x01,       // Usage Minimum (01h)
    0x29, 0x0E,       // Usage Maximum (0Eh)
    0x15, 0x00,       // Logical Minimum (0)
    0x25, 0x01,       // Logical Maximum (1)
    0x75, 0x01,       // Report Size (1)
    0x95, 0x0E,       // Report Count (14)
    0x81, 0x02,       // Input (Variable)
    0x75, 0x06,       // Report Size (6)
    0x95, 0x01,       // Report Count (1)
    0x81, 0x01,       // Input (Constant)
    0x05, 0x01,       // Usage Page (Desktop)
    0x09, 0x33,       // Usage (Rx)
    0x09, 0x34,       // Usage (Ry)
    0x15, 0x00,       // Logical Minimum (0)
    0x26, 0xFF, 0x00, // Logical Maximum (255)
    0x75, 0x08,       // Report Size (8)
    0x95, 0x02,       // Report Count (2)
    0x81, 0x02,       // Input (Variable)
    0x06, 0x00, 0xFF, // Usage Page (FF00h)
    0x09, 0x20,       // Usage (20h)
    0x95, 0x03,       // Report Count (3)
    0x81, 0x02,       // Input (Variable)
    0x05, 0x01,       // Usage Page (Desktop)
    0x19, 0x40,       // Usage Minimum (40h)
    0x29, 0x42,       // Usage Maximum (42h)
    0x16, 0x00, 0x80, // Logical Minimum (-32768)
    0x26, 0x00, 0x7F, // Logical Maximum (32767)
    0x75, 0x10,       // Report Size (16)
    0x95, 0x03,       // Report Count (3)
    0x81, 0x02,       // Input (Variable)
    0x19, 0x43,       // Usage Minimum (43h)
    0x29, 0x45,       // Usage Maximum (45h)
    0x16, 0x00, 0xE0, // Logical Minimum (-8192)
    0x26, 0xFF, 0x1F, // Logical Maximum (8191)
    0x95, 0x03,       // Report Count (3)
    0x81, 0x02,       // Input (Variable)
    0x06, 0x00, 0xFF, // Usage Page (FF00h)
    0x09, 0x20,       // Usage (20h)
    0x15, 0x00,       // Logical Minimum (0)
    0x26, 0xFF, 0x00, // Logical Maximum (255)
    0x75, 0x08,       // Report Size (8)
    0x95, 0x33,       // Report Count (51) - hid-sony had 0x31, two short of the real report
    0x81, 0x02,       // Input (Variable)
    0x09, 0x21,       // Usage (21h)
    0x75, 0x08,       // Report Size (8)
    0x95, 0x4D,       // Report Count (77)
    0x91, 0x02,       // Output (Variable)
    0x85, 0x12,       // Report ID (18)
    0x09, 0x22,       // Usage (22h)
    0x95, 0x8D,       // Report Count (141)
    0x81, 0x02,       // Input (Variable)
    0x09, 0x23,       // Usage (23h)
    0x91, 0x02,       // Output (Variable)
    0x85, 0x13,       // Report ID (19)
    0x09, 0x24,       // Usage (24h)
    0x95, 0xCD,       // Report Count (205)
    0x81, 0x02,       // Input (Variable)
    0x09, 0x25,       // Usage (25h)
    0x91, 0x02,       // Output (Variable)
    0x85, 0x14,       // Report ID (20)
    0x09, 0x26,       // Usage (26h)
    0x96, 0x0D, 0x01, // Report Count (269)
    0x81, 0x02,       // Input (Variable)
    0x09, 0x27,       // Usage (27h)
    0x91, 0x02,       // Output (Variable)
    0x85, 0x15,       // Report ID (21)
    0x09, 0x28,       // Usage (28h)
    0x96, 0x4D, 0x01, // Report Count (333)
    0x81, 0x02,       // Input (Variable)
    0x09, 0x29,       // Usage (29h)
    0x91, 0x02,       // Output (Variable)
    0x85, 0x16,       // Report ID (22)
    0x09, 0x2A,       // Usage (2Ah)
    0x96, 0x8D, 0x01, // Report Count (397)
    0x81, 0x02,       // Input (Variable)
    0x09, 0x2B,       // Usage (2Bh)
    0x91, 0x02,       // Output (Variable)
    0x85, 0x17,       // Report ID (23)
    0x09, 0x2C,       // Usage (2Ch)
    0x96, 0xCD, 0x01, // Report Count (461)
    0x81, 0x02,       // Input (Variable)
    0x09, 0x2D,       // Usage (2Dh)
    0x91, 0x02,       // Output (Variable)
    0x85, 0x18,       // Report ID (24)
    0x09, 0x2E,       // Usage (2Eh)
    0x96, 0x0D, 0x02, // Report Count (525)
    0x81, 0x02,       // Input (Variable)
    0x09, 0x2F,       // Usage (2Fh)
    0x91, 0x02,       // Output (Variable)
    0x85, 0x19,       // Report ID (25)
    0x09, 0x30,       // Usage (30h)
    0x96, 0x22, 0x02, // Report Count (546)
    0x81, 0x02,       // Input (Variable)
    0x09, 0x31,       // Usage (31h)
    0x91, 0x02,       // Output (Variable)
    0x06, 0x80, 0xFF, // Usage Page (FF80h)
    0x85, 0x82,       // Report ID (130)
    0x09, 0x22,       // Usage (22h)
    0x95, 0x3F,       // Report Count (63)
    0xB1, 0x02,       // Feature (Variable)
    0x85, 0x83,       // Report ID (131)
    0x09, 0x23,       // Usage (23h)
    0xB1, 0x02,       // Feature (Variable)
    0x85, 0x84,       // Report ID (132)
    0x09, 0x24,       // Usage (24h)
    0xB1, 0x02,       // Feature (Variable)
    0x85, 0x90,       // Report ID (144)
    0x09, 0x30,       // Usage (30h)
    0xB1, 0x02,       // Feature (Variable)
    0x85, 0x91,       // Report ID (145)
    0x09, 0x31,       // Usage (31h)
    0xB1, 0x02,       // Feature (Variable)
    0x85, 0x92,       // Report ID (146)
    0x09, 0x32,       // Usage (32h)
    0xB1, 0x02,       // Feature (Variable)
    0x85, 0x93,       // Report ID (147)
    0x09, 0x33,       // Usage (33h)
    0xB1, 0x02,       // Feature (Variable)
    0x85, 0xA0,       // Report ID (160)
    0x09, 0x40,       // Usage (40h)
    0xB1, 0x02,       // Feature (Variable)
    0x85, 0xA4,       // Report ID (164)
    0x09, 0x44,       // Usage (44h)
    0xB1, 0x02,       // Feature (Variable)
    0xC0,             // End Collection
];

const INPUT_REPORT: u8 = 0x11;
/// Report bytes after the DATA header, CRC included.
const INPUT_LEN: usize = 78;

/// CRC-32 (IEEE, as zlib) over a one-byte seed - the HID header the bytes
/// travel behind - then the bytes themselves.
pub fn crc32(seed: u8, bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in std::iter::once(&seed).chain(bytes) {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// What changes from one report to the next besides the controls.
#[derive(Default)]
pub struct Stream {
    /// Increments every report; the controller's own counter lives in the
    /// top six bits of the third button byte.
    counter: u8,
    /// Motion-sensor clock, in the controller's 5.33 microsecond units.
    timestamp: u16,
}

impl Stream {
    /// The interrupt-channel packet: DATA/Input header, then report 0x11.
    pub fn packet(&mut self, state: &State, elapsed_us: u64) -> [u8; INPUT_LEN + 1] {
        self.counter = self.counter.wrapping_add(1) & 0x3F;
        self.timestamp = self.timestamp.wrapping_add((elapsed_us * 3 / 16) as u16);
        let mut p = [0u8; INPUT_LEN + 1];
        p[0] = 0xA1;
        p[1] = INPUT_REPORT;
        // HID + CRC present, as a real controller sets them.
        p[2] = 0xC0;
        p[3] = 0x00;
        let r = &mut p[4..];
        let [lx, ly, rx, ry] = state.sticks;
        r[..4].copy_from_slice(&[lx, ly, rx, ry]);
        r[4] = buttons0(state);
        r[5] = buttons1(state);
        r[6] = (state.pressed(button::HOME) as u8) | (self.counter << 2);
        r[7] = state.triggers[0];
        r[8] = state.triggers[1];
        r[9..11].copy_from_slice(&self.timestamp.to_le_bytes());
        // Gyro at rest, accelerometer reading one g on Y: a controller lying
        // still. (r[11] is the temperature, r[12..18] the gyro.)
        r[18..20].copy_from_slice(&0i16.to_le_bytes());
        r[20..22].copy_from_slice(&8192i16.to_le_bytes());
        r[22..24].copy_from_slice(&0i16.to_le_bytes());
        // status[0]: battery 9 of 10, no cable.
        r[29] = 0x09;
        // r[32]: no touch reports. Every touch point is marked lifted anyway,
        // for a host that reads them regardless.
        for report in 0..4 {
            let base = 33 + report * 9;
            r[base + 1] = 0x80;
            r[base + 5] = 0x80;
        }
        let crc = crc32(0xA1, &p[1..INPUT_LEN - 3]);
        p[INPUT_LEN - 3..].copy_from_slice(&crc.to_le_bytes());
        p
    }
}

/// Hat in the low nibble; square, cross, circle, triangle above it - by where
/// they sit, as the generic mode's buttons are. (Mapping by letter instead,
/// so an iPad's "A" is the handheld's A, was tried and not wanted.)
fn buttons0(s: &State) -> u8 {
    let mut b = s.hat & 0x0F;
    if s.pressed(button::WEST) {
        b |= 0x10;
    }
    if s.pressed(button::SOUTH) {
        b |= 0x20;
    }
    if s.pressed(button::EAST) {
        b |= 0x40;
    }
    if s.pressed(button::NORTH) {
        b |= 0x80;
    }
    b
}

/// L1 R1 L2 R2 Share Options L3 R3, low bit first. SELECT is Share, START
/// is Options.
fn buttons1(s: &State) -> u8 {
    [button::L1, button::R1, button::L2, button::R2, button::SELECT, button::START, button::L3, button::R3]
        .iter()
        .enumerate()
        .fold(0, |b, (bit, &n)| if s.pressed(n) { b | 1 << bit } else { b })
}

/// Gyro and accelerometer calibration: zero bias, the plus/minus readings a
/// real controller reports for its reference turn, 540 deg/s for that turn,
/// and +-1 g at 8192. `bluetooth` selects the field order, which differs.
fn calibration(id: u8, len: usize, bluetooth: bool) -> Vec<u8> {
    let (plus, minus) = (8704i16, -8704i16);
    let gyro: [i16; 6] = if bluetooth {
        [plus, plus, plus, minus, minus, minus]
    } else {
        [plus, minus, plus, minus, plus, minus]
    };
    let mut r = vec![0u8; len];
    r[0] = id;
    let fields = [0i16, 0, 0].iter().chain(&gyro).chain(&[540i16, 540, 8192, -8192, 8192, -8192, 8192, -8192]).copied();
    for (i, v) in fields.enumerate() {
        r[1 + i * 2..3 + i * 2].copy_from_slice(&v.to_le_bytes());
    }
    r
}

/// A feature report, as the host asked for it by ID - or None for one this
/// controller does not have.
pub fn feature(id: u8, own: Addr, host: Addr) -> Option<Vec<u8>> {
    let report = match id {
        0x05 => {
            let mut r = calibration(0x05, 41, true);
            let crc = crc32(0xA3, &r[..37]);
            r[37..].copy_from_slice(&crc.to_le_bytes());
            r
        }
        0x02 => calibration(0x02, 37, false),
        0xA3 => {
            let mut r = vec![0u8; 49];
            r[0] = 0xA3;
            r[1..12].copy_from_slice(b"Sep 21 2018");
            r[17..25].copy_from_slice(b"04:50:51");
            r[35..37].copy_from_slice(&0x0100u16.to_le_bytes());
            r[41..43].copy_from_slice(&0x0001u16.to_le_bytes());
            r
        }
        0x12 => {
            // Own address, then the host's, both least significant first.
            let mut r = vec![0x12];
            r.extend(own.0.iter().rev());
            r.extend([0x08, 0x25, 0x00]);
            r.extend(host.0.iter().rev());
            r
        }
        // Declared by the descriptor; nothing a host relies on is in them.
        0x03 => vec![0; 39],
        0x04 => vec![0; 47],
        0x06 => vec![0; 53],
        0x07 => vec![0; 49],
        0x08 => vec![0; 48],
        0xF2 => vec![0; 16],
        0xF0 | 0xF1 | 0x82 | 0x83 | 0x84 | 0x90 | 0x91 | 0x92 | 0x93 | 0xA0 | 0xA4 => vec![0; 64],
        _ => return None,
    };
    let mut report = report;
    report[0] = id;
    Some(report)
}

const OUTPUT_REPORT: u8 = 0x11;

/// The rumble a host asked for in an output report, as one strength: the
/// handheld has a single motor where the controller has two, so the stronger
/// of the two wins. None when the report does not touch the motors (it may
/// set only the lightbar).
///
/// `report` starts at the report ID, as it arrives behind the DATA/Output
/// header on the interrupt channel or a SET_REPORT on the control one. Layout
/// as `struct dualshock4_output_report_bt` in Linux's `hid-playstation.c`:
/// ID, two control bytes, then flags, a reserved byte, right (weak) motor,
/// left (strong) motor.
pub fn rumble(report: &[u8]) -> Option<u8> {
    if report.first() != Some(&OUTPUT_REPORT) || report.len() < 8 {
        return None;
    }
    let motors_valid = report[3] & 0x01 != 0;
    motors_valid.then(|| report[6].max(report[7]))
}

/// Answer a control-channel message in this mode. GET_REPORT for a feature
/// is what differs from the generic gamepad; everything else is the same.
pub fn control(message: &[u8], own: Addr, host: Addr, state: &State) -> hid::Control {
    let Some(&header) = message.first() else { return hid::Control::Ignore };
    if header >> 4 != 0x4 {
        return hid::control(message, state);
    }
    let kind = header & 0x03;
    // With bit 3 set, a two-byte limit on the reply follows the ID.
    let limit = (header & 0x08 != 0)
        .then(|| message.get(2..4).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize))
        .flatten();
    let Some(&id) = message.get(1) else { return hid::Control::Reply(vec![hid::HANDSHAKE_INVALID_REPORT_ID]) };
    let report = match kind {
        3 => feature(id, own, host),
        // The current input report, without its DATA header.
        1 if id == INPUT_REPORT => Some(Stream::default().packet(state, 0)[1..].to_vec()),
        _ => None,
    };
    match report {
        Some(mut r) => {
            if let Some(limit) = limit {
                r.truncate(limit);
            }
            let mut reply = vec![0xA0 | kind];
            reply.extend(r);
            hid::Control::Reply(reply)
        }
        None => hid::Control::Reply(vec![hid::HANDSHAKE_INVALID_REPORT_ID]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWN: Addr = Addr([0x02, 0x11, 0x22, 0x33, 0x44, 0x55]);
    const HOST: Addr = Addr([0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC]);

    #[test]
    fn rumble_is_read_only_when_the_report_says_the_motors_are_set() {
        let mut r = vec![0u8; 78];
        r[0] = 0x11;
        r[1] = 0xC0;
        r[3] = 0x01 | 0x02;
        r[6] = 40;
        r[7] = 200;
        assert_eq!(rumble(&r), Some(200));
        r[3] = 0x02; // lightbar only
        assert_eq!(rumble(&r), None);
        r[3] = 0x01;
        r[6] = 0;
        r[7] = 0;
        assert_eq!(rumble(&r), Some(0), "a stop is a strength too");
        assert_eq!(rumble(&[0x05, 0, 0]), None);
    }

    #[test]
    fn crc_matches_zlib() {
        // zlib.crc32(b"\xa1123456789")
        assert_eq!(crc32(0xA1, b"123456789"), 0x88ED_2411);
    }

    #[test]
    fn the_input_report_has_the_size_and_crc_linux_checks() {
        let p = Stream::default().packet(&State::default(), 0);
        assert_eq!(p.len(), 79);
        assert_eq!(&p[..2], &[0xA1, 0x11]);
        let crc = u32::from_le_bytes(p[75..79].try_into().unwrap());
        assert_eq!(crc, crc32(0xA1, &p[1..75]));
    }

    #[test]
    fn controls_land_where_the_ps4_report_keeps_them() {
        let mut s = State::default();
        s.sticks = [1, 2, 3, 4];
        s.triggers = [200, 100];
        s.hat = 2;
        s.set_button(button::SOUTH, true); // cross
        s.set_button(button::NORTH, true); // triangle
        s.set_button(button::R1, true);
        s.set_button(button::START, true); // options
        s.set_button(button::HOME, true); // PS
        let p = Stream::default().packet(&s, 0);
        let r = &p[4..];
        assert_eq!(&r[..4], &[1, 2, 3, 4]);
        assert_eq!(r[4], 0x02 | 0x20 | 0x80);
        assert_eq!(r[5], 0x02 | 0x20);
        assert_eq!(r[6] & 0x01, 0x01);
        assert_eq!(&r[7..9], &[200, 100]);
    }

    #[test]
    fn face_buttons_go_by_position() {
        let face = |n: u8| {
            let mut s = State::default();
            s.set_button(n, true);
            buttons0(&s) & 0xF0
        };
        assert_eq!(face(button::SOUTH), 0x20, "bottom is cross");
        assert_eq!(face(button::EAST), 0x40, "right is circle");
        assert_eq!(face(button::WEST), 0x10, "left is square");
        assert_eq!(face(button::NORTH), 0x80, "top is triangle");
    }

    #[test]
    fn a_resting_pad_reports_a_centred_hat() {
        let p = Stream::default().packet(&State::default(), 0);
        assert_eq!(p[8] & 0x0F, 8);
    }

    #[test]
    fn the_counter_and_clock_advance_with_each_report() {
        let mut s = Stream::default();
        let a = s.packet(&State::default(), 0);
        let b = s.packet(&State::default(), 1600);
        assert_eq!((b[10] >> 2), (a[10] >> 2) + 1);
        assert_eq!(u16::from_le_bytes([b[13], b[14]]) - u16::from_le_bytes([a[13], a[14]]), 300);
    }

    #[test]
    fn bluetooth_calibration_carries_a_crc_over_the_feature_header() {
        let r = feature(0x05, OWN, HOST).unwrap();
        assert_eq!(r.len(), 41);
        assert_eq!(r[0], 0x05);
        assert_eq!(u32::from_le_bytes(r[37..41].try_into().unwrap()), crc32(0xA3, &r[..37]));
        // Linux divides by (plus - bias) + (minus - bias) for each gyro axis.
        let field = |at: usize| i16::from_le_bytes([r[at], r[at + 1]]);
        assert_eq!(field(7), 8704, "pitch plus");
        assert_eq!(field(13), -8704, "pitch minus, after the three pluses");
        assert_eq!(field(19), 540);
    }

    #[test]
    fn feature_reports_have_the_sizes_linux_expects() {
        assert_eq!(feature(0x02, OWN, HOST).unwrap().len(), 37);
        assert_eq!(feature(0xA3, OWN, HOST).unwrap().len(), 49);
        let pairing = feature(0x12, OWN, HOST).unwrap();
        assert_eq!(pairing.len(), 16);
        assert_eq!(&pairing[1..7], &[0x55, 0x44, 0x33, 0x22, 0x11, 0x02]);
        assert_eq!(feature(0x55, OWN, HOST), None);
    }

    #[test]
    fn get_report_is_answered_with_a_data_header_and_the_limit_honoured() {
        let s = State::default();
        // GET_REPORT | size follows | feature, ID 5, limit 41.
        let hid::Control::Reply(r) = control(&[0x4B, 0x05, 41, 0], OWN, HOST, &s) else { panic!() };
        assert_eq!(r[0], 0xA3);
        assert_eq!(r.len(), 42);
        let hid::Control::Reply(r) = control(&[0x4B, 0xA3, 10, 0], OWN, HOST, &s) else { panic!() };
        assert_eq!(r.len(), 11, "cut to the host's buffer");
        let hid::Control::Reply(r) = control(&[0x43, 0x77], OWN, HOST, &s) else { panic!() };
        assert_eq!(r, vec![hid::HANDSHAKE_INVALID_REPORT_ID]);
        // Not GET_REPORT: same as the generic gamepad.
        assert_eq!(control(&[0x71], OWN, HOST, &s), hid::Control::Reply(vec![0x00]));
    }

    #[test]
    fn the_descriptor_declares_report_0x11_at_the_size_sent() {
        // Bits declared under Report ID 0x11 for Input items.
        let (mut size, mut count, mut id, mut bits) = (0u32, 0u32, 0u8, 0u32);
        let mut i = 0;
        while i < DESCRIPTOR.len() {
            let prefix = DESCRIPTOR[i];
            let len = match prefix & 0x03 { 3 => 4, n => n as usize };
            let value = DESCRIPTOR[i + 1..i + 1 + len].iter().rev().fold(0u32, |v, b| v << 8 | *b as u32);
            match prefix & 0xFC {
                0x74 => size = value,
                0x94 => count = value,
                0x84 => id = value as u8,
                0x80 if id == 0x11 => bits += size * count,
                _ => {}
            }
            i += 1 + len;
        }
        assert_eq!(bits / 8, (INPUT_LEN - 1) as u32, "report bytes after the ID");
    }
}
