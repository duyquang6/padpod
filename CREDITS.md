# Credits

Padpod stands on other people's work: device documentation, the Linux
drivers that record what these controllers say about themselves, and
open-source tools. Thank you to everyone below.

## In this repository

| Work | License | What Padpod uses | Where |
|---|---|---|---|
| [Nunito](https://github.com/googlefonts/nunito), by The Nunito Project Authors | SIL Open Font License 1.1 | The interface font, subset | `crates/brick/assets/` (with `OFL.txt`) |

The DualShock 4 and Xbox Wireless Controller descriptors in
`apps/padpod/src/ds4.rs` and `apps/padpod/src/xbox.rs` are what those
controllers report about themselves, as recorded by the Linux kernel's
`hid-sony.c` and `hid-playstation.c` and by
[atar-axis/xpadneo](https://github.com/atar-axis/xpadneo)'s descriptor dumps.

## References

- The Linux kernel's `hid-playstation.c` and `hid-sony.c` - DualShock 4 report
  layouts and calibration.
- [atar-axis/xpadneo](https://github.com/atar-axis/xpadneo) - the Xbox
  Wireless Controller over Bluetooth.
- The Bluetooth HID profile specification - the service record, and the
  control and interrupt channels.
- The SPRUCE theme of spruceOS - the colours the interface uses.

## Tools Padpod runs

Not part of this repository; Padpod uses them as the handheld provides them.

- [BlueZ](http://www.bluez.org/) - the Bluetooth stack, driven over D-Bus.
- [spruceOS](https://github.com/spruceUI/spruceOS) - the handheld's firmware
  the app runs on, and whose helper scripts it calls.
- The Rust crates in `Cargo.lock` (MIT, Apache-2.0, Zlib, Unicode-3.0), each
  under its own license.

The font's license is in `crates/brick/assets/OFL.txt`.
