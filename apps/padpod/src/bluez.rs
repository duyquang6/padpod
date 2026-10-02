//! BlueZ, borrowed for the session.
//!
//! The system's `bluetoothd` runs with its `input` plugin, which is the HID
//! *host* side and holds L2CAP channels 17 and 19 - exactly the two a HID
//! *device* has to listen on. So for as long as the app runs, the system's
//! daemon is stopped and one of our own runs without that plugin, as a gamepad
//! (class 0x002508). The system's comes back on exit, and `launch.sh` does the
//! same after a crash, from the marker left in `/tmp`.
//!
//! Pairings are kept: both daemons store link keys in the same place.

use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use zbus::blocking::Connection;
use zbus::zvariant::{ObjectPath, OwnedFd, OwnedObjectPath, Value};

const SYSTEM_INIT: &str = "/etc/bluetooth/bluetoothd";
const DAEMON: &str = "/usr/bin/bluetoothd";
const CONFIG: &str = "/tmp/padpod-bluetooth.conf";
/// Present while our daemon stands in for the system's. Holds "1" when the
/// system's was running and has to be started again.
pub const MARKER: &str = "/tmp/padpod-bluetoothd";

const AGENT_PATH: &str = "/padpod/agent";
const PROFILE_PATH: &str = "/padpod/profile";

fn daemon_running() -> bool {
    Command::new("pidof").arg("bluetoothd").stdout(Stdio::null()).status().is_ok_and(|s| s.success())
}

fn stop_all_daemons() {
    let _ = Command::new(SYSTEM_INIT).arg("stop").stdout(Stdio::null()).stderr(Stdio::null()).status();
    let _ = Command::new("killall").arg("bluetoothd").stderr(Stdio::null()).status();
    let deadline = Instant::now() + Duration::from_secs(3);
    while daemon_running() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Put the system's daemon back if a session left it stopped. `launch.sh`
/// runs this after the app exits, whatever the exit was.
pub fn restore_system_daemon() {
    let Ok(was) = std::fs::read_to_string(MARKER) else { return };
    // Sniff mode back on (see `take_over`): the controller keeps its link
    // policy across daemons, so a crash would otherwise leave it off.
    let _ = Command::new("hciconfig").args(["hci0", "lp", "RSWITCH,SNIFF"]).status();
    stop_all_daemons();
    let _ = std::fs::remove_file(CONFIG);
    if was.trim() == "1" {
        let _ = Command::new(SYSTEM_INIT).arg("start").stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    let _ = std::fs::remove_file(MARKER);
}

pub struct Daemon {
    child: Child,
}

impl Daemon {
    /// `device_id` is the Device ID record's `usb:VVVV:PPPP:vvvv`, or None to
    /// keep BlueZ's own.
    pub fn take_over(name: &str, device_id: Option<String>) -> Result<Self, String> {
        if !std::path::Path::new("/sys/class/bluetooth/hci0").exists() {
            return Err("Bluetooth is off. Turn it on in the firmware's settings and open Padpod again.".into());
        }
        let was_running = daemon_running();
        std::fs::write(MARKER, if was_running { "1" } else { "0" }).map_err(|e| e.to_string())?;
        stop_all_daemons();
        // Peripheral, gamepad. The name is set here too so even the very first
        // inquiry answer carries it.
        let mut config = format!(
            "[General]\nName = {name}\nClass = 0x002508\nDiscoverableTimeout = 0\nPairableTimeout = 0\nJustWorksRepairing = always\n"
        );
        if let Some(id) = device_id {
            config.push_str(&format!("DeviceID = {id}\n"));
        }
        std::fs::write(CONFIG, config).map_err(|e| e.to_string())?;
        let _ = Command::new("hciconfig").args(["hci0", "up"]).status();
        // No sniff mode. Hosts put a gamepad's link into it to save power,
        // and then the radio only wakes every few tens of milliseconds - which
        // is felt as input lag. Refusing it costs the handheld some battery,
        // the right trade for a controller. Applies to links made from here
        // on; Drop puts the controller's default back.
        let _ = Command::new("hciconfig").args(["hci0", "lp", "RSWITCH"]).status();
        let child = Command::new(DAEMON)
            .args(["-n", "-P", "input,hog", "-f", CONFIG])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("could not start bluetoothd: {e}"))?;
        Ok(Self { child })
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        restore_system_daemon();
    }
}

/// Accepts every pairing: the app is only discoverable while its own screen
/// says so, and a gamepad has no way to show or type a code.
struct Agent;

#[zbus::interface(name = "org.bluez.Agent1")]
impl Agent {
    fn release(&self) {}
    fn request_pin_code(&self, _device: OwnedObjectPath) -> String {
        "0000".into()
    }
    fn display_pin_code(&self, _device: OwnedObjectPath, _pin: String) {}
    fn request_passkey(&self, _device: OwnedObjectPath) -> u32 {
        0
    }
    fn display_passkey(&self, _device: OwnedObjectPath, _passkey: u32, _entered: u16) {}
    fn request_confirmation(&self, device: OwnedObjectPath, _passkey: u32) {
        eprintln!("bluez: pairing with {}", device.as_str());
    }
    fn request_authorization(&self, _device: OwnedObjectPath) {}
    fn authorize_service(&self, _device: OwnedObjectPath, _uuid: String) {}
    fn cancel(&self) {}
}

/// The HID profile object. BlueZ needs one to publish the service record; the
/// channels themselves are our own sockets, so connections never come here.
struct Profile;

#[zbus::interface(name = "org.bluez.Profile1")]
impl Profile {
    fn release(&self) {}
    fn new_connection(&self, device: OwnedObjectPath, _fd: OwnedFd, _properties: HashMap<String, zbus::zvariant::OwnedValue>) {
        eprintln!("bluez: unexpected profile connection from {}", device.as_str());
    }
    fn request_disconnection(&self, _device: OwnedObjectPath) {}
}

pub struct Bus {
    conn: Connection,
}

impl Bus {
    /// Connect, wait for our daemon to come up, and register the agent and the
    /// service record. The connection is kept for the session: BlueZ drops
    /// both registrations the moment it closes.
    pub fn open(record: &str) -> Result<Self, String> {
        let conn = zbus::blocking::connection::Builder::system()
            .and_then(|b| b.serve_at(AGENT_PATH, Agent))
            .and_then(|b| b.serve_at(PROFILE_PATH, Profile))
            .and_then(|b| b.build())
            .map_err(|e| format!("could not connect to D-Bus: {e}"))?;
        let bus = Self { conn };

        // The daemon takes a moment to claim its name and find the adapter.
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            match bus.set_adapter("Powered", Value::from(true)) {
                Ok(()) => break,
                Err(e) if Instant::now() >= deadline => return Err(format!("bluetoothd is not answering: {e}")),
                Err(_) => std::thread::sleep(Duration::from_millis(200)),
            }
        }

        let agent = ObjectPath::try_from(AGENT_PATH).unwrap();
        bus.call("/org/bluez", "org.bluez.AgentManager1", "RegisterAgent", &(&agent, "NoInputNoOutput"))?;
        bus.call("/org/bluez", "org.bluez.AgentManager1", "RequestDefaultAgent", &(&agent,))?;

        let mut options: HashMap<&str, Value> = HashMap::new();
        options.insert("ServiceRecord", Value::from(record));
        options.insert("Role", Value::from("server"));
        options.insert("RequireAuthentication", Value::from(false));
        options.insert("RequireAuthorization", Value::from(false));
        let profile = ObjectPath::try_from(PROFILE_PATH).unwrap();
        bus.call(
            "/org/bluez",
            "org.bluez.ProfileManager1",
            "RegisterProfile",
            &(&profile, crate::sdp::HID_UUID, options),
        )?;
        Ok(bus)
    }

    fn call<B: serde::Serialize + zbus::zvariant::DynamicType>(
        &self,
        path: &str,
        interface: &str,
        method: &str,
        body: &B,
    ) -> Result<(), String> {
        self.conn
            .call_method(Some("org.bluez"), path, Some(interface), method, body)
            .map(|_| ())
            .map_err(|e| format!("{method}: {e}"))
    }

    fn set_adapter(&self, property: &str, value: Value) -> Result<(), String> {
        self.call(
            "/org/bluez/hci0",
            "org.freedesktop.DBus.Properties",
            "Set",
            &("org.bluez.Adapter1", property, value),
        )
    }

    pub fn set_alias(&self, name: &str) -> Result<(), String> {
        self.set_adapter("Alias", Value::from(name))
    }

    /// Visible to hosts looking for new devices, and accepting pairings.
    pub fn set_discoverable(&self, on: bool) -> Result<(), String> {
        self.set_adapter("Pairable", Value::from(on))?;
        self.set_adapter("Discoverable", Value::from(on))
    }

    fn device_path(addr: crate::l2cap::Addr) -> String {
        format!("/org/bluez/hci0/dev_{}", addr.to_string().replace(':', "_"))
    }

    /// Mark a host trusted, so it can reconnect without being asked again.
    pub fn trust(&self, addr: crate::l2cap::Addr) {
        let _ = self.call(
            &Self::device_path(addr),
            "org.freedesktop.DBus.Properties",
            "Set",
            &("org.bluez.Device1", "Trusted", Value::from(true)),
        );
    }

    /// The adapter's own address.
    pub fn address(&self) -> Option<crate::l2cap::Addr> {
        let reply = self
            .conn
            .call_method(
                Some("org.bluez"),
                "/org/bluez/hci0",
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &("org.bluez.Adapter1", "Address"),
            )
            .ok()?;
        let value: zbus::zvariant::OwnedValue = reply.body().deserialize().ok()?;
        crate::l2cap::Addr::parse(&String::try_from(value).ok()?)
    }

    /// The host's own name, as it announced itself.
    pub fn name_of(&self, addr: crate::l2cap::Addr) -> Option<String> {
        let reply = self
            .conn
            .call_method(
                Some("org.bluez"),
                Self::device_path(addr).as_str(),
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &("org.bluez.Device1", "Alias"),
            )
            .ok()?;
        let value: zbus::zvariant::OwnedValue = reply.body().deserialize().ok()?;
        String::try_from(value).ok()
    }

    /// Forget a host that unplugged us, so it can pair afresh.
    pub fn remove(&self, addr: crate::l2cap::Addr) {
        if let Ok(path) = ObjectPath::try_from(Self::device_path(addr)) {
            let _ = self.call("/org/bluez/hci0", "org.bluez.Adapter1", "RemoveDevice", &(&path,));
        }
    }
}
