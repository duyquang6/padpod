// Copyright 2026 ligt (https://github.com/duyquang6/padpod)
// SPDX-License-Identifier: GPL-3.0-only

//! The HID service record, in the XML form BlueZ's `RegisterProfile` takes.
//!
//! This is what a host reads before connecting: that the device is a gamepad,
//! which L2CAP channels to open (17 control, 19 interrupt), and the report
//! descriptor itself. The attribute layout follows the HID profile; the IDs
//! are its own (0x0200-0x020E).

use std::fmt::Write;

pub const HID_UUID: &str = "00001124-0000-1000-8000-00805f9b34fb";
pub const PSM_CONTROL: u16 = 0x11;
pub const PSM_INTERRUPT: u16 = 0x13;

pub fn record(name: &str, descriptor: &[u8]) -> String {
    let hex = descriptor.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    });
    let name = escape(name);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" ?>
<record>
  <attribute id="0x0001"><sequence><uuid value="0x1124" /></sequence></attribute>
  <attribute id="0x0004">
    <sequence>
      <sequence><uuid value="0x0100" /><uint16 value="0x0011" /></sequence>
      <sequence><uuid value="0x0011" /></sequence>
    </sequence>
  </attribute>
  <attribute id="0x0005"><sequence><uuid value="0x1002" /></sequence></attribute>
  <attribute id="0x0006">
    <sequence><uint16 value="0x656e" /><uint16 value="0x006a" /><uint16 value="0x0100" /></sequence>
  </attribute>
  <attribute id="0x0009">
    <sequence><sequence><uuid value="0x1124" /><uint16 value="0x0101" /></sequence></sequence>
  </attribute>
  <attribute id="0x000d">
    <sequence>
      <sequence>
        <sequence><uuid value="0x0100" /><uint16 value="0x0013" /></sequence>
        <sequence><uuid value="0x0011" /></sequence>
      </sequence>
    </sequence>
  </attribute>
  <attribute id="0x0100"><text value="{name}" /></attribute>
  <attribute id="0x0101"><text value="Gamepad" /></attribute>
  <attribute id="0x0102"><text value="Padpod" /></attribute>
  <attribute id="0x0200"><uint16 value="0x0100" /></attribute>
  <attribute id="0x0201"><uint16 value="0x0111" /></attribute>
  <attribute id="0x0202"><uint8 value="0x08" /></attribute>
  <attribute id="0x0203"><uint8 value="0x00" /></attribute>
  <attribute id="0x0204"><boolean value="true" /></attribute>
  <attribute id="0x0205"><boolean value="true" /></attribute>
  <attribute id="0x0206">
    <sequence><sequence><uint8 value="0x22" /><text encoding="hex" value="{hex}" /></sequence></sequence>
  </attribute>
  <attribute id="0x0207">
    <sequence><sequence><uint16 value="0x0409" /><uint16 value="0x0100" /></sequence></sequence>
  </attribute>
  <attribute id="0x020b"><uint16 value="0x0100" /></attribute>
  <attribute id="0x020c"><uint16 value="0x0c80" /></attribute>
  <attribute id="0x020d"><boolean value="false" /></attribute>
  <attribute id="0x020e"><boolean value="false" /></attribute>
</record>
"#
    )
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_record_carries_the_descriptor_as_hex_and_both_channels() {
        let xml = record("Padpod", &[0x05, 0x01, 0xC0]);
        assert!(xml.contains(r#"<text encoding="hex" value="0501c0" />"#));
        assert!(xml.contains(r#"<uint16 value="0x0011" />"#), "control PSM");
        assert!(xml.contains(r#"<uint16 value="0x0013" />"#), "interrupt PSM");
        assert!(xml.contains(r#"<attribute id="0x0202"><uint8 value="0x08" />"#), "gamepad subclass");
    }

    #[test]
    fn a_name_cannot_break_out_of_its_attribute() {
        let xml = record(r#"a"<b>&"#, &[]);
        assert!(xml.contains("a&quot;&lt;b&gt;&amp;"));
    }
}
