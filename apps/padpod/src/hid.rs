// Copyright 2026 ligt (https://github.com/duyquang6/padpod)
// SPDX-License-Identifier: GPL-3.0-only

//! The gamepad as a Bluetooth HID device sees it: the report descriptor the
//! host reads from the service record, the input report sent on every change,
//! and the answers owed on the control channel.
//!
//! The report has no report ID, so on the interrupt channel it is the DATA
//! header `0xA1` followed by these nine bytes:
//!
//! | byte | contents |
//! |---|---|
//! | 0-1 | buttons 1-16, one bit each |
//! | 2 | hat switch (low nibble, 8 = centred), high nibble unused |
//! | 3-6 | left stick X, Y (X, Y); right stick X, Y (Rx, Ry); 0-255, centre 128 |
//! | 7-8 | left and right trigger (Z, Rz); 0-255 |
//!
//! This is the axis layout of Linux's own Xbox driver, which is what SDL (and
//! so Steam and most games) and browsers on Linux expect of a gamepad they do
//! not know: Z/Rz are read as the triggers. Measured: with the sticks on
//! Z/Rz instead, a browser on Linux showed the right stick as L2/R2.

/// Button usages in Linux's gamepad order: hid-input maps button N of a
/// Game Pad collection to `BTN_GAMEPAD + N - 1`, so this order is what makes a
/// Linux host see BTN_SOUTH for the bottom face button, and SDL and Steam
/// build their mappings on top of that.
pub const DESCRIPTOR: &[u8] = &[
    0x05, 0x01, //       Usage Page (Generic Desktop)
    0x09, 0x05, //       Usage (Game Pad)
    0xA1, 0x01, //       Collection (Application)
    0x05, 0x09, //         Usage Page (Button)
    0x19, 0x01, //         Usage Minimum (1)
    0x29, 0x10, //         Usage Maximum (16)
    0x15, 0x00, //         Logical Minimum (0)
    0x25, 0x01, //         Logical Maximum (1)
    0x75, 0x01, //         Report Size (1)
    0x95, 0x10, //         Report Count (16)
    0x81, 0x02, //         Input (Data, Variable, Absolute)
    0x05, 0x01, //         Usage Page (Generic Desktop)
    0x09, 0x39, //         Usage (Hat Switch)
    0x15, 0x00, //         Logical Minimum (0)
    0x25, 0x07, //         Logical Maximum (7)
    0x35, 0x00, //         Physical Minimum (0)
    0x46, 0x3B, 0x01, //   Physical Maximum (315)
    0x65, 0x14, //         Unit (degrees)
    0x75, 0x04, //         Report Size (4)
    0x95, 0x01, //         Report Count (1)
    0x81, 0x42, //         Input (Data, Variable, Absolute, Null State)
    0x65, 0x00, //         Unit (none)
    0x75, 0x04, //         Report Size (4)
    0x95, 0x01, //         Report Count (1)
    0x81, 0x03, //         Input (Constant) - padding
    0x09, 0x30, //         Usage (X)  - left stick
    0x09, 0x31, //         Usage (Y)
    0x09, 0x33, //         Usage (Rx) - right stick
    0x09, 0x34, //         Usage (Ry)
    0x09, 0x32, //         Usage (Z)  - left trigger
    0x09, 0x35, //         Usage (Rz) - right trigger
    0x15, 0x00, //         Logical Minimum (0)
    0x26, 0xFF, 0x00, //   Logical Maximum (255)
    0x75, 0x08, //         Report Size (8)
    0x95, 0x06, //         Report Count (6)
    0x81, 0x02, //         Input (Data, Variable, Absolute)
    0xC0, //             End Collection
];

pub const REPORT_LEN: usize = 9;

/// Button numbers (1-based, as the descriptor counts them).
#[allow(dead_code)]
pub mod button {
    pub const SOUTH: u8 = 1;
    pub const EAST: u8 = 2;
    pub const NORTH: u8 = 4;
    pub const WEST: u8 = 5;
    pub const L1: u8 = 7;
    pub const R1: u8 = 8;
    pub const L2: u8 = 9;
    pub const R2: u8 = 10;
    pub const SELECT: u8 = 11;
    pub const START: u8 = 12;
    pub const HOME: u8 = 13;
    pub const L3: u8 = 14;
    pub const R3: u8 = 15;
}

/// Hat value for "nothing pressed": outside the logical range, which the
/// Null State flag tells the host to read as centred.
pub const HAT_CENTRED: u8 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct State {
    pub buttons: u16,
    pub hat: u8,
    /// Left X, left Y, right X, right Y.
    pub sticks: [u8; 4],
    pub triggers: [u8; 2],
}

impl Default for State {
    fn default() -> Self {
        Self { buttons: 0, hat: HAT_CENTRED, sticks: [128; 4], triggers: [0; 2] }
    }
}

impl State {
    pub fn set_button(&mut self, number: u8, down: bool) {
        let bit = 1u16 << (number - 1);
        if down {
            self.buttons |= bit;
        } else {
            self.buttons &= !bit;
        }
    }

    pub fn pressed(&self, number: u8) -> bool {
        self.buttons & (1 << (number - 1)) != 0
    }

    pub fn report(&self) -> [u8; REPORT_LEN] {
        let [b0, b1] = self.buttons.to_le_bytes();
        let [lx, ly, rx, ry] = self.sticks;
        let [lt, rt] = self.triggers;
        [b0, b1, self.hat & 0x0F, lx, ly, rx, ry, lt, rt]
    }
}

/// The hat value for a D-pad direction, from the two hat axes as evdev reports
/// them (-1, 0 or 1; negative is up and left). 0 is up, counting clockwise.
pub fn hat(x: i32, y: i32) -> u8 {
    match (x.signum(), y.signum()) {
        (0, -1) => 0,
        (1, -1) => 1,
        (1, 0) => 2,
        (1, 1) => 3,
        (0, 1) => 4,
        (-1, 1) => 5,
        (-1, 0) => 6,
        (-1, -1) => 7,
        _ => HAT_CENTRED,
    }
}

/// The interrupt-channel packet for a report: DATA, Input.
pub fn input_packet(state: &State) -> [u8; REPORT_LEN + 1] {
    let mut packet = [0u8; REPORT_LEN + 1];
    packet[0] = 0xA1;
    packet[1..].copy_from_slice(&state.report());
    packet
}

const HANDSHAKE_SUCCESSFUL: u8 = 0x00;
pub const HANDSHAKE_INVALID_REPORT_ID: u8 = 0x02;
const HANDSHAKE_UNSUPPORTED: u8 = 0x03;
const HANDSHAKE_INVALID_PARAMETER: u8 = 0x04;

/// What a control-channel message asks of us.
#[derive(Debug, PartialEq, Eq)]
pub enum Control {
    /// Send these bytes back on the control channel.
    Reply(Vec<u8>),
    /// The host has unpaired: forget it and disconnect.
    Unplug,
    /// Nothing to answer (a suspend notice, say).
    Ignore,
}

/// Answer one message from the host, per the HID profile's transaction types.
pub fn control(message: &[u8], state: &State) -> Control {
    let Some(&header) = message.first() else { return Control::Ignore };
    let param = header & 0x0F;
    match header >> 4 {
        // HID_CONTROL: 3 suspend, 4 exit suspend, 5 virtual cable unplug.
        0x1 if param == 5 => Control::Unplug,
        0x1 => Control::Ignore,
        // GET_REPORT. Only an input report exists; the low two bits say which
        // type was asked for.
        0x4 if param & 0x03 == 1 => {
            let mut reply = vec![0xA1];
            reply.extend_from_slice(&state.report());
            Control::Reply(reply)
        }
        0x4 => Control::Reply(vec![HANDSHAKE_INVALID_PARAMETER]),
        // SET_REPORT: there is nothing to set (no rumble, no LEDs), but
        // refusing it makes some hosts give up on the device.
        0x5 => Control::Reply(vec![HANDSHAKE_SUCCESSFUL]),
        // GET_PROTOCOL: always the report protocol.
        0x6 => Control::Reply(vec![0xA0, 0x01]),
        // SET_PROTOCOL and SET_IDLE are acknowledged; the report never
        // changes shape and every change is sent anyway.
        0x7 | 0x9 => Control::Reply(vec![HANDSHAKE_SUCCESSFUL]),
        // GET_IDLE: infinite, which is to say only on change.
        0x8 => Control::Reply(vec![0xA0, 0x00]),
        _ => Control::Reply(vec![HANDSHAKE_UNSUPPORTED]),
    }
}

/// Scale an axis reading from its device range onto 0-255.
pub fn scale(value: i32, min: i32, max: i32) -> u8 {
    if max <= min {
        return 128;
    }
    let clamped = value.clamp(min, max) as i64;
    ((clamped - min as i64) * 255 / (max as i64 - min as i64)) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_descriptor_declares_exactly_the_bytes_the_report_carries() {
        // Sum Report Size * Report Count over the Input items.
        let mut bits = 0u32;
        let (mut size, mut count) = (0u32, 0u32);
        let mut i = 0;
        while i < DESCRIPTOR.len() {
            let prefix = DESCRIPTOR[i];
            let len = match prefix & 0x03 {
                3 => 4,
                n => n as usize,
            };
            let value = DESCRIPTOR[i + 1..i + 1 + len].iter().rev().fold(0u32, |v, b| v << 8 | *b as u32);
            match prefix & 0xFC {
                0x74 => size = value,
                0x94 => count = value,
                0x80 => bits += size * count,
                _ => {}
            }
            i += 1 + len;
        }
        assert_eq!(bits, REPORT_LEN as u32 * 8);
    }

    #[test]
    fn a_resting_pad_reports_centred_sticks_and_no_buttons() {
        assert_eq!(State::default().report(), [0, 0, HAT_CENTRED, 128, 128, 128, 128, 0, 0]);
    }

    #[test]
    fn buttons_land_on_their_own_bits() {
        let mut s = State::default();
        s.set_button(button::SOUTH, true);
        s.set_button(button::HOME, true);
        assert_eq!(s.report()[..2], [0b0000_0001, 0b0001_0000]);
        assert!(s.pressed(button::HOME));
        s.set_button(button::SOUTH, false);
        assert!(!s.pressed(button::SOUTH));
        assert_eq!(s.report()[0], 0);
    }

    #[test]
    fn the_hat_counts_clockwise_from_up() {
        assert_eq!(hat(0, -1), 0);
        assert_eq!(hat(1, 0), 2);
        assert_eq!(hat(0, 1), 4);
        assert_eq!(hat(-1, 0), 6);
        assert_eq!(hat(-1, -1), 7);
        assert_eq!(hat(0, 0), HAT_CENTRED);
    }

    #[test]
    fn the_interrupt_packet_is_a_data_input_header_and_the_report() {
        let p = input_packet(&State::default());
        assert_eq!(p[0], 0xA1);
        assert_eq!(p.len(), 10);
    }

    #[test]
    fn control_messages_get_the_answers_the_profile_requires() {
        let s = State::default();
        assert_eq!(control(&[0x15], &s), Control::Unplug);
        assert_eq!(control(&[0x13], &s), Control::Ignore);
        assert_eq!(control(&[0x71], &s), Control::Reply(vec![0x00]), "SET_PROTOCOL report");
        assert_eq!(control(&[0x90, 0x00], &s), Control::Reply(vec![0x00]), "SET_IDLE");
        assert_eq!(control(&[0x60], &s), Control::Reply(vec![0xA0, 0x01]));
        let Control::Reply(r) = control(&[0x41], &s) else { panic!() };
        assert_eq!(r[0], 0xA1);
        assert_eq!(r.len(), REPORT_LEN + 1);
        assert_eq!(control(&[0x43], &s), Control::Reply(vec![0x04]), "no feature report");
        assert_eq!(control(&[0x52, 0x01], &s), Control::Reply(vec![0x00]));
        assert_eq!(control(&[0xB0], &s), Control::Reply(vec![0x03]));
        assert_eq!(control(&[], &s), Control::Ignore);
    }

    #[test]
    fn axes_scale_onto_a_byte() {
        assert_eq!(scale(-32768, -32768, 32767), 0);
        assert_eq!(scale(32767, -32768, 32767), 255);
        assert_eq!(scale(0, -32768, 32767), 127);
        assert_eq!(scale(255, 0, 255), 255);
        assert_eq!(scale(999, 0, 255), 255, "clamped");
        assert_eq!(scale(5, 0, 0), 128, "a range that says nothing");
    }
}
