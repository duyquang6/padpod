// Copyright 2026 ligt (https://github.com/duyquang6/padpod)
// SPDX-License-Identifier: GPL-3.0-only

//! Xbox Wireless Controller (model 1708, 045E:02FD), as it speaks over
//! classic Bluetooth.
//!
//! What makes Windows hand it to games as an XInput pad, and what iPadOS and
//! Android list as an Xbox controller. The controller presents one of two
//! descriptors depending on the host it believes it is talking to; this is
//! the Windows one, whose axes - sticks on X/Y and Rx/Ry, triggers on Z/Rz -
//! are also what Linux and SDL expect of a gamepad. Descriptor as xpadneo's
//! documentation dumps it (docs/descriptors/xb1s_windows.md).
//!
//! Reports:
//! - 0x01: sticks (16 bit, 0-65535), triggers (10 bit), hat (1-8, 0 at
//!   rest), ten buttons - A B X Y LB RB View Menu LS RS.
//! - 0x02: the Xbox button, alone.
//! - 0x03 (from the host): rumble, four magnitudes of 0-100.

use crate::hid::{button, State, HAT_CENTRED};

pub const VENDOR: u16 = 0x045E;
pub const PRODUCT: u16 = 0x02FD;
pub const NAME: &str = "Xbox Wireless Controller";

pub const DESCRIPTOR: &[u8] = &[
    0x05, 0x01, 0x09, 0x05, 0xA1, 0x01, 0x85, 0x01, 0x09, 0x01, 0xA1, 0x00,
    0x09, 0x30, 0x09, 0x31, 0x15, 0x00, 0x27, 0xFF, 0xFF, 0x00, 0x00, 0x95,
    0x02, 0x75, 0x10, 0x81, 0x02, 0xC0, 0x09, 0x01, 0xA1, 0x00, 0x09, 0x33,
    0x09, 0x34, 0x15, 0x00, 0x27, 0xFF, 0xFF, 0x00, 0x00, 0x95, 0x02, 0x75,
    0x10, 0x81, 0x02, 0xC0, 0x05, 0x01, 0x09, 0x32, 0x15, 0x00, 0x26, 0xFF,
    0x03, 0x95, 0x01, 0x75, 0x0A, 0x81, 0x02, 0x15, 0x00, 0x25, 0x00, 0x75,
    0x06, 0x95, 0x01, 0x81, 0x03, 0x05, 0x01, 0x09, 0x35, 0x15, 0x00, 0x26,
    0xFF, 0x03, 0x95, 0x01, 0x75, 0x0A, 0x81, 0x02, 0x15, 0x00, 0x25, 0x00,
    0x75, 0x06, 0x95, 0x01, 0x81, 0x03, 0x05, 0x01, 0x09, 0x39, 0x15, 0x01,
    0x25, 0x08, 0x35, 0x00, 0x46, 0x3B, 0x01, 0x66, 0x14, 0x00, 0x75, 0x04,
    0x95, 0x01, 0x81, 0x42, 0x75, 0x04, 0x95, 0x01, 0x15, 0x00, 0x25, 0x00,
    0x35, 0x00, 0x45, 0x00, 0x65, 0x00, 0x81, 0x03, 0x05, 0x09, 0x19, 0x01,
    0x29, 0x0A, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x0A, 0x81, 0x02,
    0x15, 0x00, 0x25, 0x00, 0x75, 0x06, 0x95, 0x01, 0x81, 0x03, 0x05, 0x01,
    0x09, 0x80, 0x85, 0x02, 0xA1, 0x00, 0x09, 0x85, 0x15, 0x00, 0x25, 0x01,
    0x95, 0x01, 0x75, 0x01, 0x81, 0x02, 0x15, 0x00, 0x25, 0x00, 0x75, 0x07,
    0x95, 0x01, 0x81, 0x03, 0xC0, 0x05, 0x0F, 0x09, 0x21, 0x85, 0x03, 0xA1,
    0x02, 0x09, 0x97, 0x15, 0x00, 0x25, 0x01, 0x75, 0x04, 0x95, 0x01, 0x91,
    0x02, 0x15, 0x00, 0x25, 0x00, 0x75, 0x04, 0x95, 0x01, 0x91, 0x03, 0x09,
    0x70, 0x15, 0x00, 0x25, 0x64, 0x75, 0x08, 0x95, 0x04, 0x91, 0x02, 0x09,
    0x50, 0x66, 0x01, 0x10, 0x55, 0x0E, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x75,
    0x08, 0x95, 0x01, 0x91, 0x02, 0x09, 0xA7, 0x15, 0x00, 0x26, 0xFF, 0x00,
    0x75, 0x08, 0x95, 0x01, 0x91, 0x02, 0x65, 0x00, 0x55, 0x00, 0x09, 0x7C,
    0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x01, 0x91, 0x02, 0xC0,
    0x85, 0x04, 0x05, 0x06, 0x09, 0x20, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x75,
    0x08, 0x95, 0x01, 0x81, 0x02, 0xC0,
];

/// Report 0x01 behind its DATA header.
pub fn controls(s: &State) -> Vec<u8> {
    let mut r = vec![0xA1, 0x01];
    // 0 is up and left, as the handheld's own axes.
    for v in s.sticks {
        r.extend((v as u16 * 257).to_le_bytes());
    }
    for t in s.triggers {
        r.extend(((t as u32 * 1023 / 255) as u16).to_le_bytes());
    }
    // 1 is up, clockwise; 0 is nothing pressed.
    r.push(if s.hat == HAT_CENTRED { 0 } else { s.hat + 1 });
    // By position, as Xbox games expect: bottom is A whatever the handheld
    // prints on it.
    let order = [
        button::SOUTH,  // A
        button::EAST,   // B
        button::WEST,   // X
        button::NORTH,  // Y
        button::L1,     // LB
        button::R1,     // RB
        button::SELECT, // View
        button::START,  // Menu
        button::L3,
        button::R3,
    ];
    let bits = order.iter().enumerate().fold(0u16, |b, (i, &n)| if s.pressed(n) { b | 1 << i } else { b });
    r.extend(bits.to_le_bytes());
    r
}

/// Report 0x02: the Xbox button.
pub fn guide(s: &State) -> Vec<u8> {
    vec![0xA1, 0x02, s.pressed(button::HOME) as u8]
}

/// The rumble a host asked for, 0-255, from report 0x03 (starting at its
/// ID): the strongest of the four motors it enabled. The handheld has one.
pub fn rumble(report: &[u8]) -> Option<u8> {
    if report.first() != Some(&0x03) || report.len() < 6 {
        return None;
    }
    let enabled = report[1] & 0x0F;
    let strongest = report[2..6].iter().enumerate().filter(|(i, _)| enabled & (1 << (3 - i)) != 0).map(|(_, &m)| m).max();
    Some((strongest.unwrap_or(0).min(100) as u16 * 255 / 100) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_one_is_fifteen_bytes_as_the_descriptor_declares() {
        let r = controls(&State::default());
        assert_eq!(&r[..2], &[0xA1, 0x01]);
        assert_eq!(r.len(), 2 + 15);
        // Resting sticks sit at the middle, triggers and hat at zero.
        assert_eq!(u16::from_le_bytes([r[2], r[3]]), 128 * 257);
        assert_eq!(&r[10..15], &[0, 0, 0, 0, 0]);
    }

    #[test]
    fn controls_land_where_an_xbox_pad_keeps_them() {
        let mut s = State::default();
        s.sticks = [0, 255, 128, 128];
        s.triggers = [255, 0];
        s.hat = 2; // right
        s.set_button(button::SOUTH, true); // A
        s.set_button(button::NORTH, true); // Y
        s.set_button(button::START, true); // Menu
        let r = controls(&s);
        assert_eq!(u16::from_le_bytes([r[2], r[3]]), 0);
        assert_eq!(u16::from_le_bytes([r[4], r[5]]), 65535);
        assert_eq!(u16::from_le_bytes([r[10], r[11]]), 1023);
        assert_eq!(r[14], 3, "right is 3 counting from up = 1");
        assert_eq!(u16::from_le_bytes([r[15], r[16]]), 0b1000_1001);
    }

    #[test]
    fn the_xbox_button_has_a_report_of_its_own() {
        let mut s = State::default();
        assert_eq!(guide(&s), vec![0xA1, 0x02, 0]);
        s.set_button(button::HOME, true);
        assert_eq!(guide(&s), vec![0xA1, 0x02, 1]);
    }

    #[test]
    fn rumble_is_the_strongest_enabled_motor() {
        // Enable strong and weak (low bits), 60 and 20.
        assert_eq!(rumble(&[0x03, 0x03, 0, 0, 60, 20, 0xFF, 0, 0xEB]), Some(153));
        // A disabled motor does not count.
        assert_eq!(rumble(&[0x03, 0x01, 0, 0, 100, 20, 0, 0, 0]), Some(51));
        assert_eq!(rumble(&[0x03, 0x00, 0, 0, 100, 100, 0, 0, 0]), Some(0));
        assert_eq!(rumble(&[0x01, 0x0F]), None);
    }

    #[test]
    fn the_descriptor_declares_report_one_at_the_size_sent() {
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
                0x80 if id == 1 => bits += size * count,
                _ => {}
            }
            i += 1 + len;
        }
        assert_eq!(bits, 15 * 8);
    }
}
