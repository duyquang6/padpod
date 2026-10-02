// Copyright 2026 ligt (https://github.com/duyquang6/padpod)
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Padpod: the TrimUI Brick Pro as a Bluetooth gamepad.
//!
//! Three modes, chosen from the menu SELECT opens. **PC** (the default)
//! announces a standard HID gamepad, which Windows, Linux, macOS and Android
//! take with no driver. **Xbox** answers as an Xbox Wireless Controller, which
//! Windows hands to games as XInput. **PS4** answers as a DualShock 4, for
//! hosts that only take controllers they know - iPadOS and its games. The screen says what state the
//! link is in and lights each control as it is pressed; it dims itself once a
//! host is connected, since the point is to look at the other screen.

mod bluez;
use brick::canvas;
mod ds4;
mod hid;
mod l2cap;
mod pad;
mod rumble;
#[cfg(test)]
use brick::png;
mod sdp;
mod xbox;
use brick::text;

use canvas::{Canvas, Rgb};
use l2cap::{Addr, Channel, Listener};
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use text::Fonts;

const NAME: &str = "Padpod";
/// Who made it, and where it lives: on screen, so the credit travels with
/// the app.
const AUTHOR: &str = "ligt";
const HOME_PAGE: &str = "github.com/duyquang6/padpod";
/// The release, as `package.sh` builds it; "dev" for any other build.
const VERSION: &str = match option_env!("PADPOD_VERSION") {
    Some(v) => v,
    None => "dev",
};
const DATA_DIR: &str = "/mnt/SDCARD/Saves/padpod";
const THEME_FONT: &str = "/mnt/SDCARD/Themes/SPRUCE/nunwen.ttf";

/// Holding MENU this long quits. A tap is the gamepad's Home button.
const QUIT_HOLD: Duration = Duration::from_secs(2);
/// How long after the last change the screen stays lit while connected.
const DIM_AFTER: Duration = Duration::from_secs(20);
/// The same with the battery saver on.
const SAVER_DIM_AFTER: Duration = Duration::from_secs(8);
/// Between attempts to reach the host we were last paired with.
const RECONNECT_EVERY: Duration = Duration::from_secs(4);
/// How long after starting or losing the link to keep at that pace...
const RECONNECT_BRISKLY_FOR: Duration = Duration::from_secs(60);
/// ...and the pace after that.
const RECONNECT_SLOWLY: Duration = Duration::from_secs(30);
/// How often a DualShock 4 reports with nothing changing. A real one streams
/// continuously, and some hosts take a silent controller for a gone one.
const PS4_REPORT_EVERY: Duration = Duration::from_millis(8);

// The look follows spruce's SPRUCE theme (Gruvbox), as the handheld's other
// native apps do: black, cream text, an orange accent.
const BG: Rgb = Rgb(0, 0, 0);
/// The title, and the rule under it.
const TITLE: Rgb = Rgb(0xEB, 0xDB, 0xB2);
const FG: Rgb = Rgb(0xFB, 0xF1, 0xC7);
/// Secondary text, still bright enough to read at a glance.
const DIM: Rgb = Rgb(0xBD, 0xAE, 0x93);
/// What is pressed, chosen or on.
const ACCENT: Rgb = Rgb(0xD6, 0x5D, 0x0E);
/// The menu's cursor: the accent, dimmed.
const CURSOR: Rgb = Rgb(0x6B, 0x2F, 0x07);
/// The rule under the title: the title, dimmed.
const RULE: Rgb = Rgb(0x76, 0x6E, 0x59);
const OK: Rgb = Rgb(0xB8, 0xBB, 0x26);
const WARN: Rgb = Rgb(0xFA, 0xBD, 0x2F);
const ERROR: Rgb = Rgb(0xFB, 0x49, 0x34);
/// Every control's resting colour. The panel shows dark greys darker than a
/// monitor does: at (40, 44, 56) on the body the D-pad and sticks could not
/// be made out on the device.
const CHIP: Rgb = Rgb(0x66, 0x5C, 0x54);
/// A stick's cap, lighter than its well so where it points can be seen.
const KNOB: Rgb = Rgb(0xBD, 0xAE, 0x93);
const WELL: Rgb = Rgb(0x3C, 0x38, 0x36);

// The layout: the title and a rule along the top, the content below, the
// buttons' hints along the bottom.
const LEFT: i32 = 28;
const TITLE_BASELINE: i32 = 52;
const TITLE_SIZE: f32 = 40.0;
const RULE_Y: i32 = 70;
const CONTENT_TOP: i32 = 116;
const TEXT_SIZE: f32 = 28.0;
const BOTTOM_BASELINE: i32 = 744;
const HINT_SIZE: f32 = 26.0;

fn data_dir() -> PathBuf {
    PathBuf::from(std::env::var("PADPOD_DATA").unwrap_or_else(|_| DATA_DIR.into()))
}

/// What the handheld pretends to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Pc,
    Xbox,
    Ps4,
}

impl Mode {
    /// In the order the menu lists them.
    const ALL: [Mode; 3] = [Mode::Pc, Mode::Xbox, Mode::Ps4];

    fn label(self) -> &'static str {
        match self {
            Mode::Pc => "PC",
            Mode::Xbox => "Xbox",
            Mode::Ps4 => "PS4",
        }
    }

    /// The name hosts list it under.
    fn bluetooth_name(self) -> &'static str {
        match self {
            Mode::Pc => NAME,
            Mode::Xbox => xbox::NAME,
            Mode::Ps4 => ds4::NAME,
        }
    }

    fn descriptor(self) -> &'static [u8] {
        match self {
            Mode::Pc => hid::DESCRIPTOR,
            Mode::Xbox => xbox::DESCRIPTOR,
            Mode::Ps4 => ds4::DESCRIPTOR,
        }
    }

    /// The Device ID record: how a host knows which controller this is. The
    /// PC mode keeps BlueZ's own.
    fn device_id(self) -> Option<String> {
        match self {
            Mode::Pc => None,
            Mode::Xbox => Some(format!("usb:{:04X}:{:04X}:0408", xbox::VENDOR, xbox::PRODUCT)),
            Mode::Ps4 => Some(format!("usb:{:04X}:{:04X}:0100", ds4::VENDOR, ds4::PRODUCT)),
        }
    }

    fn load() -> Self {
        match std::fs::read_to_string(data_dir().join("mode")).as_deref().map(str::trim) {
            Ok("PS4") => Mode::Ps4,
            Ok("Xbox") => Mode::Xbox,
            _ => Mode::Pc,
        }
    }

    fn save(self) {
        let _ = std::fs::create_dir_all(data_dir());
        let _ = std::fs::write(data_dir().join("mode"), self.label());
    }
}

/// The battery saver: with a host connected, the panel turns fully off
/// sooner and the CPU drops to spruce's powersave profile while it is off.
fn saver_on() -> bool {
    std::fs::read_to_string(data_dir().join("battery")).is_ok_and(|s| s.trim() == "on")
}

fn save_saver(on: bool) {
    let _ = std::fs::create_dir_all(data_dir());
    let _ = std::fs::write(data_dir().join("battery"), if on { "on" } else { "off" });
}

/// The last host, per mode: a host caches what the controller said it was
/// when they paired, so reconnecting a mode to a host paired in the other one
/// would hand it reports it reads wrongly.
fn host_file(mode: Mode) -> PathBuf {
    data_dir().join(match mode {
        // The name from before there were modes.
        Mode::Pc => "host",
        Mode::Xbox => "host-xbox",
        Mode::Ps4 => "host-ps4",
    })
}

fn saved_host(mode: Mode) -> Option<Addr> {
    std::fs::read_to_string(host_file(mode)).ok().and_then(|s| Addr::parse(&s))
}

fn save_host(mode: Mode, addr: Option<Addr>) {
    let path = host_file(mode);
    match addr {
        Some(a) => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(path, a.to_string());
        }
        None => {
            let _ = std::fs::remove_file(path);
        }
    }
}

mod backlight {
    //! Dimming the panel while a host is connected. The raw value found at
    //! startup goes to /tmp first so `launch.sh` can put it back after a crash.
    use std::os::unix::io::AsRawFd;

    const STATE: &str = "/sys/class/disp/disp/attr/sys";
    const DEVICE: &str = "/dev/disp";
    const BACKUP: &str = "/tmp/padpod-backlight";
    const SET_BRIGHTNESS: libc::c_ulong = 0x102;

    fn read() -> Option<u32> {
        let state = std::fs::read_to_string(STATE).ok()?;
        let rest = &state[state.find("backlight(")? + "backlight(".len()..];
        rest[..rest.find(')')?].trim().parse().ok()
    }

    /// 0 is off - the ioctl takes 0..255, where spruce's helper stops at 1.
    fn set(raw: u32) {
        let Ok(device) = std::fs::OpenOptions::new().read(true).write(true).open(DEVICE) else { return };
        let mut args: [libc::c_ulong; 4] = [0, raw.min(255) as libc::c_ulong, 0, 0];
        unsafe { libc::ioctl(device.as_raw_fd(), SET_BRIGHTNESS as _, args.as_mut_ptr()) };
    }

    pub struct Backlight {
        original: Option<u32>,
        dimmed: bool,
        /// spruce's powersave CPU profile is on, and set_smart is owed.
        cpu_low: bool,
    }

    const HELPERS: &str = "/mnt/SDCARD/spruce/scripts/helperFunctions.sh";

    /// One of spruce's CPU profiles, as Truepod switches them: the helpers
    /// are shell functions and have to be sourced. Powersave takes two cores
    /// offline and lets the clock fall to 408 MHz; smart holds 1 GHz on all
    /// four, which is what launch.sh starts the app with.
    pub fn cpu(profile: &str) {
        if std::path::Path::new(HELPERS).exists() {
            let _ = std::process::Command::new("sh")
                .arg("-c")
                .arg(format!(". {HELPERS}; {profile}"))
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }

    impl Backlight {
        pub fn open() -> Self {
            let original = read();
            if let Some(raw) = original {
                let _ = std::fs::write(BACKUP, raw.to_string());
            }
            Self { original, dimmed: false, cpu_low: false }
        }

        /// Dim the panel, or light it again. `off` turns it fully off rather
        /// than to its faintest glow, and drops the CPU to spruce's
        /// powersave profile while it stays dark - the battery saver.
        pub fn dim(&mut self, on: bool, off: bool) {
            if on == self.dimmed {
                return;
            }
            if let Some(raw) = self.original {
                set(match (on, off) {
                    (false, _) => raw,
                    (true, true) => 0,
                    (true, false) => 1,
                });
                self.dimmed = on;
            }
            match (on, off) {
                (true, true) => cpu("set_powersave"),
                (false, _) if self.cpu_low => cpu("set_smart"),
                _ => {}
            }
            self.cpu_low = on && off;
        }

        pub fn is_dimmed(&self) -> bool {
            self.dimmed
        }
    }

    impl Drop for Backlight {
        fn drop(&mut self) {
            self.dim(false, false);
            let _ = std::fs::remove_file(BACKUP);
        }
    }

    pub fn restore_from_backup() {
        if let Some(raw) = std::fs::read_to_string(BACKUP).ok().and_then(|s| s.trim().parse().ok()) {
            set(raw);
            // The backup exists only while the app ran, so a crash may have
            // left the CPU in powersave too.
            cpu("set_smart");
        }
        let _ = std::fs::remove_file(BACKUP);
    }
}

struct Link {
    control: Channel,
    interrupt: Channel,
    host: Addr,
    name: String,
}

#[derive(Clone)]
enum Status {
    Starting,
    Failed(String),
    /// `reconnecting` names the host being reconnected to.
    Waiting { reconnecting: Option<String>, mode: Mode },
    Connected { name: String, mode: Mode },
}

/// Keep trying to reach the host last paired with, while nothing is connected.
/// A HID device is expected to reconnect on its own; a PC does not come
/// looking for its gamepad.
///
/// Briskly for the first minute after starting or losing the link, then only
/// now and then. Each attempt pages the host for seconds, and a radio busy
/// paging answers scans poorly: trying every few seconds for a host that is
/// switched off left the handheld hard to find for pairing anything else.
fn spawn_reconnector(
    target: Arc<Mutex<Option<Addr>>>,
    connected: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    out: mpsc::Sender<(Channel, Channel, Addr)>,
) {
    std::thread::spawn(move || {
        let mut since = Instant::now();
        let mut last_try: Option<Instant> = None;
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(250));
            if connected.load(Ordering::Relaxed) {
                since = Instant::now();
                continue;
            }
            let every = if since.elapsed() < RECONNECT_BRISKLY_FOR { RECONNECT_EVERY } else { RECONNECT_SLOWLY };
            if last_try.is_some_and(|t| t.elapsed() < every) {
                continue;
            }
            last_try = Some(Instant::now());
            let Some(host) = *target.lock().unwrap() else { continue };
            let Ok(control) = Channel::connect(host, sdp::PSM_CONTROL) else { continue };
            // A session that ended while this was paging must not be handed
            // a link meant for the mode it was in.
            if stop.load(Ordering::Relaxed) {
                return;
            }
            match Channel::connect(host, sdp::PSM_INTERRUPT) {
                Ok(interrupt) => {
                    eprintln!("link: reconnected to {host}");
                    if out.send((control, interrupt, host)).is_err() {
                        return;
                    }
                }
                Err(e) => eprintln!("link: {host} took the control channel but not the interrupt one: {e}"),
            }
        }
    });
}

/// Ends the session's reconnector when the session ends, however it ends.
struct StopOnDrop(Arc<AtomicBool>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// What the screen should show next.
struct Frame {
    status: Status,
    state: hid::State,
    quitting: bool,
    menu: Option<Menu>,
    /// The panel is dimmed: draw nothing but black.
    blank: bool,
}

/// The screen, drawn on a thread of its own.
///
/// Drawing a frame takes long enough on this CPU that doing it between
/// reading the pad and sending the report put every redraw in the path of
/// the next input: a stick moving continuously queued its events behind the
/// frames it caused. The I/O loop now only posts the latest state here; a
/// frame that is still drawing when several more arrive is followed by the
/// newest only.
struct Renderer {
    slot: Arc<(Mutex<Option<Option<Frame>>>, std::sync::Condvar)>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// The shortest time between two frames. The screen is a status display; the
/// gamepad is what has to be quick.
const FRAME_EVERY: Duration = Duration::from_millis(33);

impl Renderer {
    fn spawn(mut canvas: Canvas, mut fonts: Fonts) -> Self {
        let slot: Arc<(Mutex<Option<Option<Frame>>>, std::sync::Condvar)> = Arc::default();
        let shared = Arc::clone(&slot);
        let thread = std::thread::spawn(move || {
            // Below the I/O thread, so a frame never delays a report.
            unsafe { libc::setpriority(libc::PRIO_PROCESS, libc::gettid() as _, 10) };
            let (lock, ready) = &*shared;
            let mut drawn = 0u32;
            loop {
                let next = {
                    let mut pending = lock.lock().unwrap();
                    while pending.is_none() {
                        pending = ready.wait(pending).unwrap();
                    }
                    pending.take().unwrap()
                };
                let started = Instant::now();
                match next {
                    None => {
                        canvas.clear(Rgb(0, 0, 0));
                        let _ = canvas.flush();
                        return;
                    }
                    Some(frame) if frame.blank => {
                        canvas.clear(Rgb(0, 0, 0));
                        let _ = canvas.flush();
                    }
                    Some(frame) => {
                        draw(&mut canvas, &mut fonts, &frame.status, &frame.state, frame.quitting);
                        if let Some(menu) = frame.menu {
                            draw_menu(&mut canvas, &mut fonts, &menu);
                        }
                        let _ = canvas.flush();
                        // A few, to know what a frame costs on the device.
                        drawn += 1;
                        if drawn <= 5 {
                            eprintln!("screen: frame {drawn} took {} ms", started.elapsed().as_millis());
                        }
                    }
                }
                if let Some(rest) = FRAME_EVERY.checked_sub(started.elapsed()) {
                    std::thread::sleep(rest);
                }
            }
        });
        Self { slot, thread: Some(thread) }
    }

    fn show(&self, frame: Frame) {
        let (lock, ready) = &*self.slot;
        *lock.lock().unwrap() = Some(Some(frame));
        ready.notify_one();
    }
}

impl Drop for Renderer {
    /// Blank the screen and wait for it, so the launcher never comes back
    /// over a half-drawn frame.
    fn drop(&mut self) {
        let (lock, ready) = &*self.slot;
        *lock.lock().unwrap() = Some(None);
        ready.notify_one();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("restore") {
        bluez::restore_system_daemon();
        backlight::restore_from_backup();
        // A crash mid-rumble leaves the motor spinning until something
        // writes the pin.
        drop(rumble::Rumble::open());
        return;
    }
    if let Err(e) = run() {
        eprintln!("padpod: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let canvas = Canvas::open("/dev/fb0").map_err(|e| format!("framebuffer: {e}"))?;
    let fonts = Fonts::load(&[THEME_FONT.to_string()]);
    let mut pad = pad::Pad::open(&pad::pad_path()).map_err(|e| format!("pad: {e}"))?;
    let screen = Renderer::spawn(canvas, fonts);
    let mut light = backlight::Backlight::open();
    let motor = rumble::Rumble::open();
    if motor.is_none() {
        eprintln!("padpod: no rumble motor found");
    }
    let mut mode = Mode::load();
    loop {
        match session(mode, &screen, &mut pad, &mut light, motor.as_ref())? {
            Outcome::Quit => break,
            Outcome::Switch(next) => {
                mode = next;
                mode.save();
                eprintln!("padpod: mode {}", mode.label());
            }
        }
    }
    drop(screen);
    Ok(())
}

enum Outcome {
    Quit,
    /// Start over as another controller: it has its own name, IDs and
    /// service record.
    Switch(Mode),
}

fn frame(status: Status, state: hid::State, quitting: bool) -> Frame {
    Frame { status, state, quitting, menu: None, blank: false }
}

/// The menu SELECT opens while nothing is connected: the three modes, then
/// the battery saver. While a host is connected SELECT is that host's button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Menu {
    cursor: usize,
    current: Mode,
    saver: bool,
}

/// Rows: the modes, then the saver.
const MENU_ROWS: usize = Mode::ALL.len() + 1;

/// What a press does to the open menu.
#[derive(Debug, PartialEq, Eq)]
enum MenuAction {
    Stay,
    Close,
    Switch(Mode),
    ToggleSaver,
}

/// Presses are edges: `was` is the state before this frame.
fn menu_press(menu: &mut Menu, was: &hid::State, now: &hid::State) -> MenuAction {
    use hid::button;
    let pressed = |n: u8| now.pressed(n) && !was.pressed(n);
    let hat_to = |dir: u8| now.hat == dir && was.hat != dir;
    if hat_to(0) {
        menu.cursor = (menu.cursor + MENU_ROWS - 1) % MENU_ROWS;
    } else if hat_to(4) {
        menu.cursor = (menu.cursor + 1) % MENU_ROWS;
    } else if pressed(button::EAST) {
        // A: the handheld's right-hand button.
        return match Mode::ALL.get(menu.cursor) {
            Some(&m) if m == menu.current => MenuAction::Close,
            Some(&m) => MenuAction::Switch(m),
            None => MenuAction::ToggleSaver,
        };
    } else if pressed(button::SOUTH) || pressed(button::SELECT) {
        return MenuAction::Close;
    }
    MenuAction::Stay
}

/// Bluetooth up as one mode's controller, until MENU is held or the mode is
/// changed.
fn session(
    mode: Mode,
    screen: &Renderer,
    pad: &mut pad::Pad,
    light: &mut backlight::Backlight,
    motor: Option<&rumble::Rumble>,
) -> Result<Outcome, String> {
    let rumble = |level: u8| {
        if let Some(m) = motor {
            m.set(level);
        }
    };
    screen.show(frame(Status::Starting, pad.mapper.state, false));
    light.dim(false, false);

    let daemon = match bluez::Daemon::take_over(mode.bluetooth_name(), mode.device_id()) {
        Ok(d) => d,
        Err(e) => return fail(screen, pad, e).map(|()| Outcome::Quit),
    };
    let bus = match bluez::Bus::open(&sdp::record(mode.bluetooth_name(), mode.descriptor())) {
        Ok(b) => b,
        Err(e) => return fail(screen, pad, e).map(|()| Outcome::Quit),
    };
    let _ = bus.set_alias(mode.bluetooth_name());
    let own = bus.address().unwrap_or(Addr([0; 6]));
    let listeners = Listener::bind(sdp::PSM_CONTROL).and_then(|c| Ok((c, Listener::bind(sdp::PSM_INTERRUPT)?)));
    let (control_listener, interrupt_listener) = match listeners {
        Ok(pair) => pair,
        Err(e) => return fail(screen, pad, format!("could not open the HID channels: {e}")).map(|()| Outcome::Quit),
    };
    let _ = control_listener.set_nonblocking();
    let _ = interrupt_listener.set_nonblocking();

    let target = Arc::new(Mutex::new(saved_host(mode)));
    let connected = Arc::new(AtomicBool::new(false));
    let (reconnected_tx, reconnected) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let _stop_reconnector = StopOnDrop(Arc::clone(&stop));
    spawn_reconnector(Arc::clone(&target), Arc::clone(&connected), stop, reconnected_tx);

    let mut link: Option<Link> = None;
    // A control channel accepted, waiting for its interrupt channel.
    let mut half: Option<(Channel, Addr)> = None;
    let mut discoverable = None;
    let mut sent = None;
    let mut sent_at = Instant::now();
    let mut stream = ds4::Stream::default();
    let mut changed_at = Instant::now();
    let mut home_since: Option<Instant> = None;
    let mut previous = pad.mapper.state;
    let mut menu: Option<Menu> = None;
    let mut saver = saver_on();
    let mut drawn: Option<(String, hid::State, bool)> = None;
    let mut names: std::collections::HashMap<Addr, String> = Default::default();

    let outcome = loop {
        // Discoverable only while nothing is connected: a gamepad in use has
        // no reason to answer every scan in the room.
        let want_discoverable = link.is_none();
        if discoverable != Some(want_discoverable) {
            if let Err(e) = bus.set_discoverable(want_discoverable) {
                eprintln!("bluez: {e}");
            }
            discoverable = Some(want_discoverable);
        }

        let mut fds = vec![
            libc::pollfd { fd: pad.as_raw_fd(), events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: control_listener.as_raw_fd(), events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: interrupt_listener.as_raw_fd(), events: libc::POLLIN, revents: 0 },
        ];
        if let Some(l) = &link {
            fds.push(libc::pollfd { fd: l.control.as_raw_fd(), events: libc::POLLIN, revents: 0 });
            fds.push(libc::pollfd { fd: l.interrupt.as_raw_fd(), events: libc::POLLIN, revents: 0 });
        }
        // A DualShock 4 is due its next report on a timer; everything else
        // waits for something to happen.
        let wait = match (mode, &link) {
            (Mode::Ps4, Some(_)) => PS4_REPORT_EVERY.saturating_sub(sent_at.elapsed()).as_millis() as i32,
            _ => 100,
        };
        unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, wait) };

        if fds[0].revents & libc::POLLIN != 0 {
            match pad.read() {
                Ok(true) => changed_at = Instant::now(),
                Ok(false) => {}
                Err(e) => return Err(format!("pad: {e}")),
            }
        }

        // New connections: the host opens control first, then interrupt.
        if let Ok(Some((control, host))) = control_listener.accept() {
            eprintln!("link: control channel from {host}");
            half = Some((control, host));
        }
        if let Ok(Some((interrupt, host))) = interrupt_listener.accept() {
            match half.take() {
                Some((control, from)) if from == host => {
                    link = Some(Link { control, interrupt, name: String::new(), host });
                }
                _ => eprintln!("link: interrupt channel from {host} with no control channel; dropped"),
            }
        }
        if let Ok((control, interrupt, host)) = reconnected.try_recv() {
            if link.is_none() {
                link = Some(Link { control, interrupt, name: String::new(), host });
            }
        }
        if let Some(l) = link.as_mut().filter(|l| l.name.is_empty()) {
            // Just connected.
            bus.trust(l.host);
            save_host(mode, Some(l.host));
            *target.lock().unwrap() = Some(l.host);
            connected.store(true, Ordering::Relaxed);
            l.name = bus.name_of(l.host).unwrap_or_else(|| l.host.to_string());
            eprintln!("link: connected to {} ({}) as {}", l.name, l.host, mode.label());
            sent = None;
            changed_at = Instant::now();
        }

        // The host's side of the conversation.
        let mut lost = false;
        let mut unplugged = false;
        if let Some(l) = &link {
            loop {
                match l.control.recv() {
                    Ok(Some(message)) => {
                        // SET_REPORT carrying an output report: the other
                        // way a host can send rumble.
                        if let Some(&0x52) = message.first() {
                            let level = match mode {
                                Mode::Ps4 => ds4::rumble(&message[1..]),
                                Mode::Xbox => xbox::rumble(&message[1..]),
                                Mode::Pc => None,
                            };
                            if let Some(level) = level {
                                rumble(level);
                            }
                        }
                        let answer = match mode {
                            Mode::Pc | Mode::Xbox => hid::control(&message, &pad.mapper.state),
                            Mode::Ps4 => ds4::control(&message, own, l.host, &pad.mapper.state),
                        };
                        match answer {
                            hid::Control::Reply(bytes) => {
                                if l.control.send(&bytes).is_err() {
                                    lost = true;
                                }
                            }
                            hid::Control::Unplug => unplugged = true,
                            hid::Control::Ignore => {}
                        }
                    }
                    Ok(None) => break,
                    Err(_) => {
                        lost = true;
                        break;
                    }
                }
            }
            // Output reports: a DualShock 4's rumble is acted on, its
            // lightbar ignored. A closed channel shows up here as a read.
            loop {
                match l.interrupt.recv() {
                    Ok(Some(message)) => {
                        if let Some(&0xA2) = message.first() {
                            let level = match mode {
                                Mode::Ps4 => ds4::rumble(&message[1..]),
                                Mode::Xbox => xbox::rumble(&message[1..]),
                                Mode::Pc => None,
                            };
                            if let Some(level) = level {
                                rumble(level);
                            }
                        }
                    }
                    Ok(None) => break,
                    Err(_) => {
                        lost = true;
                        break;
                    }
                }
            }
            let due = match mode {
                Mode::Pc | Mode::Xbox => Some(pad.mapper.state) != sent,
                Mode::Ps4 => Some(pad.mapper.state) != sent || sent_at.elapsed() >= PS4_REPORT_EVERY,
            };
            if !lost && due {
                let result = match mode {
                    Mode::Pc => l.interrupt.send(&hid::input_packet(&pad.mapper.state)),
                    // The Xbox button travels in a report of its own, sent
                    // only when it changes.
                    Mode::Xbox => {
                        let home = hid::button::HOME;
                        let guide_changed = sent.is_none_or(|s: hid::State| s.pressed(home) != pad.mapper.state.pressed(home));
                        l.interrupt.send(&xbox::controls(&pad.mapper.state)).and_then(|()| {
                            if guide_changed {
                                l.interrupt.send(&xbox::guide(&pad.mapper.state))
                            } else {
                                Ok(())
                            }
                        })
                    }
                    Mode::Ps4 => {
                        let elapsed = sent_at.elapsed().as_micros() as u64;
                        l.interrupt.send(&stream.packet(&pad.mapper.state, elapsed))
                    }
                };
                match result {
                    Ok(()) => {
                        sent = Some(pad.mapper.state);
                        sent_at = Instant::now();
                    }
                    // A full queue is the radio falling behind; the next
                    // report carries the whole state anyway.
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(_) => lost = true,
                }
            }
        }
        if unplugged {
            if let Some(l) = link.take() {
                eprintln!("link: {} unpaired us", l.name);
                bus.remove(l.host);
            }
            save_host(mode, None);
            *target.lock().unwrap() = None;
            lost = true;
        }
        if lost {
            if let Some(l) = link.take() {
                eprintln!("link: {} disconnected", l.name);
            }
            rumble(0);
            connected.store(false, Ordering::Relaxed);
            changed_at = Instant::now();
        }

        // MENU held: quit, after letting go of everything on the host.
        home_since = match (pad.home_held(), home_since) {
            (true, None) => Some(Instant::now()),
            (true, Some(t)) => Some(t),
            (false, _) => None,
        };
        if home_since.is_some_and(|t| t.elapsed() >= QUIT_HOLD) {
            // In the mode's own format: a PS4 host reads a generic report as
            // garbage and would keep MENU (the PS button) held.
            if let Some(l) = &link {
                let rest = hid::State::default();
                let _ = match mode {
                    Mode::Pc => l.interrupt.send(&hid::input_packet(&rest)),
                    Mode::Xbox => l
                        .interrupt
                        .send(&xbox::controls(&rest))
                        .and_then(|()| l.interrupt.send(&xbox::guide(&rest))),
                    Mode::Ps4 => l.interrupt.send(&stream.packet(&rest, 0)),
                };
            }
            break Outcome::Quit;
        }

        // SELECT opens the menu, but only with nothing connected: while a
        // host is listening it is that host's Back/Share/View button.
        let now = pad.mapper.state;
        if link.is_some() {
            menu = None;
        } else if let Some(open) = menu.as_mut() {
            match menu_press(open, &previous, &now) {
                MenuAction::Stay => {}
                MenuAction::Close => menu = None,
                MenuAction::Switch(next) => break Outcome::Switch(next),
                MenuAction::ToggleSaver => {
                    saver = !saver;
                    save_saver(saver);
                    open.saver = saver;
                }
            }
        } else if now.pressed(hid::button::SELECT) && !previous.pressed(hid::button::SELECT) {
            let cursor = Mode::ALL.iter().position(|&m| m == mode).unwrap_or(0);
            menu = Some(Menu { cursor, current: mode, saver });
        }
        previous = now;

        let status = match &link {
            Some(l) => Status::Connected { name: l.name.clone(), mode },
            None => {
                // By name, as the host announced itself; asked of BlueZ once.
                let reconnecting = (*target.lock().unwrap())
                    .map(|a| names.entry(a).or_insert_with(|| bus.name_of(a).unwrap_or_else(|| a.to_string())).clone());
                Status::Waiting { reconnecting, mode }
            }
        };
        let dim_after = if saver { SAVER_DIM_AFTER } else { DIM_AFTER };
        let should_dim = link.is_some() && changed_at.elapsed() >= dim_after;
        light.dim(should_dim, saver);
        if !light.is_dimmed() {
            let key = (format!("{} {menu:?} {saver}", status_key(&status)), pad.mapper.state, home_since.is_some());
            if drawn.as_ref() != Some(&key) {
                screen.show(Frame { menu, ..frame(status, pad.mapper.state, home_since.is_some()) });
                drawn = Some(key);
            }
        } else if drawn.is_some() {
            screen.show(Frame { blank: true, ..frame(status, pad.mapper.state, false) });
            drawn = None;
        }
    };

    drop(link);
    rumble(0);
    let _ = bus.set_discoverable(false);
    drop(bus);
    drop(daemon);
    Ok(outcome)
}

fn status_key(status: &Status) -> String {
    match status {
        Status::Starting => "starting".into(),
        Status::Failed(e) => format!("failed {e}"),
        Status::Waiting { reconnecting, mode } => format!("waiting {reconnecting:?} {mode:?}"),
        Status::Connected { name, mode } => format!("connected {name} {mode:?}"),
    }
}

/// Show an error and wait for MENU or B, so it can be read before the app
/// closes.
fn fail(screen: &Renderer, pad: &mut pad::Pad, error: String) -> Result<(), String> {
    eprintln!("padpod: {error}");
    screen.show(Frame { status: Status::Failed(error.clone()), state: pad.mapper.state, quitting: false, menu: None, blank: false });
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        let mut fd = libc::pollfd { fd: pad.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        unsafe { libc::poll(&mut fd, 1, 200) };
        if fd.revents & libc::POLLIN != 0 && pad.read().is_ok() {
            let s = &pad.mapper.state;
            if s.pressed(hid::button::HOME) || s.pressed(hid::button::SOUTH) {
                break;
            }
        }
    }
    drop(error);
    Ok(())
}

#[derive(Clone, Copy)]
enum Control {
    /// A hat direction, true for it and for the two diagonals beside it.
    Hat(u8),
}

fn lit(control: Control, state: &hid::State) -> bool {
    match control {
        Control::Hat(dir) => {
            state.hat != hid::HAT_CENTRED && matches!((state.hat as i32 - dir as i32).rem_euclid(8), 0 | 1 | 7)
        }
    }
}

/// Coverage for a rounded rectangle, anti-aliased on its edge. A circle is
/// the case where the radius is half the side.
fn round_rect_mask(w: i32, h: i32, r: i32) -> Vec<u8> {
    let (w, h) = (w.max(1), h.max(1));
    let r = (r as f32).min(w as f32 / 2.0).min(h as f32 / 2.0);
    let (hw, hh) = (w as f32 / 2.0, h as f32 / 2.0);
    let mut mask = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let qx = ((x as f32 + 0.5 - hw).abs() - (hw - r)).max(0.0);
            let qy = ((y as f32 + 0.5 - hh).abs() - (hh - r)).max(0.0);
            let d = (qx * qx + qy * qy).sqrt() - r;
            mask.push(((0.5 - d).clamp(0.0, 1.0) * 255.0) as u8);
        }
    }
    mask
}

fn round_rect(canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32, r: i32, colour: Rgb) {
    // The same few shapes every frame; their masks are worked out once.
    thread_local! {
        static MASKS: std::cell::RefCell<std::collections::HashMap<(i32, i32, i32), Vec<u8>>> = Default::default();
    }
    MASKS.with(|masks| {
        let mut masks = masks.borrow_mut();
        let mask = masks.entry((w, h, r)).or_insert_with(|| round_rect_mask(w, h, r));
        canvas.blend_mask(x, y, w.max(1) as usize, mask, colour);
    });
}

fn circle(canvas: &mut Canvas, cx: i32, cy: i32, r: i32, colour: Rgb) {
    round_rect(canvas, cx - r, cy - r, 2 * r, 2 * r, r, colour);
}

const BODY: Rgb = Rgb(0x28, 0x28, 0x28);
const BODY_EDGE: Rgb = Rgb(0x50, 0x49, 0x45);
const SHOULDER_BACK: Rgb = Rgb(0x50, 0x49, 0x45);

/// A shoulder button, `visible` pixels of it above the body: the front one
/// (L1/R1) solid, the back one (L2/R2) filling up with how far its trigger is
/// pulled. The tab runs on under the body, which is drawn over it.
fn shoulder(canvas: &mut Canvas, fonts: &mut Fonts, x: i32, y: i32, w: i32, visible: i32, label: &str, on: bool, travel: Option<u8>) {
    let base = if travel.is_some() { SHOULDER_BACK } else { CHIP };
    round_rect(canvas, x, y, w, visible + 40, 18, if on { ACCENT } else { base });
    if let (Some(t), false) = (travel, on) {
        let filled = visible * t as i32 / 255;
        if filled > 0 {
            canvas.fill_rect(x + 10, y + visible - filled, w - 20, filled, CURSOR);
        }
    }
    fonts.draw_centred(canvas, label, x + w / 2, y + visible / 2 + 9, 26.0, if on { BG } else { FG });
}

fn face_button(canvas: &mut Canvas, fonts: &mut Fonts, cx: i32, cy: i32, label: &str, on: bool) {
    circle(canvas, cx, cy, 44, if on { ACCENT } else { CHIP });
    fonts.draw_centred(canvas, label, cx, cy + 12, 34.0, if on { BG } else { FG });
}

fn pill(canvas: &mut Canvas, fonts: &mut Fonts, cx: i32, cy: i32, w: i32, label: &str, on: bool) {
    round_rect(canvas, cx - w / 2, cy - 20, w, 40, 20, if on { ACCENT } else { CHIP });
    fonts.draw_centred(canvas, label, cx, cy + 8, 20.0, if on { BG } else { FG });
}

/// The D-pad as a cross; each arm lights for its direction and the
/// diagonals beside it.
/// An analog stick: its well, and the cap shown where the stick is pushed.
fn stick(canvas: &mut Canvas, cx: i32, cy: i32, x: u8, y: u8, clicked: bool) {
    circle(canvas, cx, cy, 54, CHIP);
    circle(canvas, cx, cy, 48, WELL);
    let offset = |v: u8| (v as i32 - 128) * 26 / 128;
    circle(canvas, cx + offset(x), cy + offset(y), 30, if clicked { ACCENT } else { KNOB });
}


/// The title and the rule under it, with a short reading at the right of
/// the title's line.
fn header(canvas: &mut Canvas, fonts: &mut Fonts, title: &str, corner: &str, corner_colour: Rgb) {
    let w = canvas.width();
    fonts.draw(canvas, title, LEFT, TITLE_BASELINE, TITLE_SIZE, TITLE);
    let size = TITLE_SIZE * 0.75;
    let room = w - LEFT * 2 - fonts.measure(title, TITLE_SIZE) - 24;
    let corner = fonts.ellipsize(corner, size, room);
    let cw = fonts.measure(&corner, size);
    fonts.draw(canvas, &corner, w - LEFT - cw, TITLE_BASELINE, size, corner_colour);
    canvas.fill_rect(LEFT, RULE_Y, w - LEFT * 2, 2, RULE);
}

/// Button hints along the bottom - each key in the accent, what it does
/// beside it - and the credit at the right.
fn footer(canvas: &mut Canvas, fonts: &mut Fonts, hints: &[(&str, &str)], credit: &str) {
    let w = canvas.width();
    let mut x = LEFT;
    for (i, (key, does)) in hints.iter().enumerate() {
        if i > 0 {
            x += 26;
        }
        x = fonts.draw(canvas, key, x, BOTTOM_BASELINE, HINT_SIZE, ACCENT) + 8;
        x = fonts.draw(canvas, does, x, BOTTOM_BASELINE, HINT_SIZE, DIM);
    }
    let size = HINT_SIZE * 0.85;
    let credit = fonts.ellipsize(credit, size, w - LEFT - x - 32);
    let cw = fonts.measure(&credit, size);
    fonts.draw(canvas, &credit, w - LEFT - cw, BOTTOM_BASELINE, size, DIM);
}

/// Lines of text under the rule: the first in the text colour, the rest
/// dimmer.
fn lines(canvas: &mut Canvas, fonts: &mut Fonts, text: &[String]) {
    let w = canvas.width();
    let mut y = CONTENT_TOP;
    for (i, line) in text.iter().enumerate() {
        for wrapped in fonts.wrap(line, TEXT_SIZE, w - LEFT * 2, 2) {
            fonts.draw(canvas, &wrapped, LEFT, y, TEXT_SIZE, if i == 0 { FG } else { DIM });
            y += 36;
        }
    }
}

/// A line `width` wide from one point to another, as a run of dots: enough
/// for the few short strokes of the face buttons' symbols.
fn stroke(canvas: &mut Canvas, (x0, y0): (i32, i32), (x1, y1): (i32, i32), width: i32, colour: Rgb) {
    let steps = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
    for i in 0..=steps {
        circle(canvas, x0 + (x1 - x0) * i / steps, y0 + (y1 - y0) * i / steps, width / 2, colour);
    }
}

/// A round face button `r` across; returns the colour to mark it in.
fn round_button(canvas: &mut Canvas, cx: i32, cy: i32, r: i32, on: bool) -> Rgb {
    circle(canvas, cx, cy, r, if on { ACCENT } else { CHIP });
    if on { BG } else { FG }
}

/// The D-pad as a cross, `arm` wide and reaching `reach` from its centre;
/// each arm lights for its direction and the diagonals beside it.
fn dpad_sized(canvas: &mut Canvas, cx: i32, cy: i32, arm: i32, reach: i32, state: &hid::State) {
    let r = arm / 5;
    round_rect(canvas, cx - arm / 2, cy - reach, arm, 2 * reach, r, CHIP);
    round_rect(canvas, cx - reach, cy - arm / 2, 2 * reach, arm, r, CHIP);
    let len = reach - arm / 2 - 4;
    let arms = [
        (0, cx - arm / 2, cy - reach, arm, len),
        (4, cx - arm / 2, cy + arm / 2 + 4, arm, len),
        (6, cx - reach, cy - arm / 2, len, arm),
        (2, cx + arm / 2 + 4, cy - arm / 2, len, arm),
    ];
    for (dir, x, y, w, h) in arms {
        if lit(Control::Hat(dir), state) {
            round_rect(canvas, x, y, w, h, r, ACCENT);
        }
    }
    circle(canvas, cx, cy, arm / 5, WELL);
}

/// The face of an Xbox-style controller: an offset layout, the left stick
/// above the D-pad and the face buttons above the right stick. Drawn plain,
/// with no maker's logo: the Home button is a plain disc.
fn xbox_face(canvas: &mut Canvas, fonts: &mut Fonts, state: &hid::State) {
    use hid::button;
    // Bumpers in front, triggers behind them; the body overlaps both.
    shoulder(canvas, fonts, 210, 196, 150, 60, "LT", state.pressed(button::L2), Some(state.triggers[0]));
    shoulder(canvas, fonts, 664, 196, 150, 60, "RT", state.pressed(button::R2), Some(state.triggers[1]));
    shoulder(canvas, fonts, 150, 236, 220, 56, "LB", state.pressed(button::L1), None);
    shoulder(canvas, fonts, 654, 236, 220, 56, "RB", state.pressed(button::R1), None);
    // The body: a wide top and two grips.
    for (grow, colour) in [(3, BODY_EDGE), (0, BODY)] {
        round_rect(canvas, 110 - grow, 280 - grow, 804 + 2 * grow, 270 + 2 * grow, 130 + grow, colour);
        circle(canvas, 250, 548, 140 + grow, colour);
        circle(canvas, 774, 548, 140 + grow, colour);
    }
    // Plain disc for Home, View and Menu beside it.
    circle(canvas, 512, 330, 30, if state.pressed(button::HOME) { ACCENT } else { CHIP });
    pill(canvas, fonts, 440, 410, 70, "VIEW", state.pressed(button::SELECT));
    pill(canvas, fonts, 584, 410, 70, "MENU", state.pressed(button::START));
    let [lx, ly, rx, ry] = state.sticks;
    stick(canvas, 290, 395, lx, ly, state.pressed(button::L3));
    dpad_sized(canvas, 390, 545, 54, 82, state);
    stick(canvas, 634, 545, rx, ry, state.pressed(button::R3));
    // A, B, X, Y by position, as Xbox games read them: the bottom button is
    // A whatever the handheld prints on it.
    let (fx, fy, gap) = (744, 395, 66);
    for (label, x, y, n) in [
        ("Y", fx, fy - gap, button::NORTH),
        ("X", fx - gap, fy, button::WEST),
        ("B", fx + gap, fy, button::EAST),
        ("A", fx, fy + gap, button::SOUTH),
    ] {
        let on = state.pressed(n);
        let ink = round_button(canvas, x, y, 34, on);
        fonts.draw_centred(canvas, label, x, y + 11, 32.0, if on { ink } else { letter_colour(label) });
    }
}

/// The colours Xbox players know the letters by, in the theme's own
/// shades.
fn letter_colour(label: &str) -> Rgb {
    match label {
        "A" => OK,
        "B" => ERROR,
        "X" => Rgb(0x83, 0xA5, 0x98),
        _ => WARN,
    }
}

/// The face of a DualShock-style controller: both sticks low and side by
/// side, a touchpad between the D-pad and the face buttons, which carry
/// shapes rather than letters. The Home button is a plain disc.
fn ds4_face(canvas: &mut Canvas, fonts: &mut Fonts, state: &hid::State) {
    use hid::button;
    shoulder(canvas, fonts, 180, 196, 160, 60, "L2", state.pressed(button::L2), Some(state.triggers[0]));
    shoulder(canvas, fonts, 684, 196, 160, 60, "R2", state.pressed(button::R2), Some(state.triggers[1]));
    shoulder(canvas, fonts, 150, 236, 200, 52, "L1", state.pressed(button::L1), None);
    shoulder(canvas, fonts, 674, 236, 200, 52, "R1", state.pressed(button::R1), None);
    // A wide top, and two long grips reaching down.
    for (grow, colour) in [(3, BODY_EDGE), (0, BODY)] {
        round_rect(canvas, 110 - grow, 276 - grow, 804 + 2 * grow, 250 + 2 * grow, 110 + grow, colour);
        round_rect(canvas, 130 - grow, 380 - grow, 250 + 2 * grow, 320 + 2 * grow, 125 + grow, colour);
        round_rect(canvas, 644 - grow, 380 - grow, 250 + 2 * grow, 320 + 2 * grow, 125 + grow, colour);
    }
    // The touchpad, Share and Options at its corners.
    round_rect(canvas, 392, 290, 240, 130, 20, WELL);
    pill(canvas, fonts, 340, 316, 84, "SHARE", state.pressed(button::SELECT));
    pill(canvas, fonts, 690, 316, 104, "OPTIONS", state.pressed(button::START));
    dpad_sized(canvas, 255, 380, 54, 84, state);
    let [lx, ly, rx, ry] = state.sticks;
    stick(canvas, 400, 530, lx, ly, state.pressed(button::L3));
    stick(canvas, 624, 530, rx, ry, state.pressed(button::R3));
    circle(canvas, 512, 500, 24, if state.pressed(button::HOME) { ACCENT } else { CHIP });
    // Shapes by position: triangle top, circle right, cross bottom, square
    // left - the same buttons the PS4 mode sends.
    let (fx, fy, gap) = (789, 380, 66);
    let shapes: [(i32, i32, u8, Rgb); 4] = [
        (fx, fy - gap, button::NORTH, OK),
        (fx + gap, fy, button::EAST, ERROR),
        (fx, fy + gap, button::SOUTH, Rgb(0x83, 0xA5, 0x98)),
        (fx - gap, fy, button::WEST, Rgb(0xD3, 0x86, 0x9B)),
    ];
    for (i, (x, y, n, tint)) in shapes.into_iter().enumerate() {
        let on = state.pressed(n);
        let ink = if on { round_button(canvas, x, y, 34, true) } else { round_button(canvas, x, y, 34, false); tint };
        let fill = if on { ACCENT } else { CHIP };
        match i {
            // Triangle.
            0 => {
                let (a, b, c) = ((x, y - 15), (x - 15, y + 11), (x + 15, y + 11));
                stroke(canvas, a, b, 5, ink);
                stroke(canvas, b, c, 5, ink);
                stroke(canvas, c, a, 5, ink);
            }
            // Circle: a ring.
            1 => {
                circle(canvas, x, y, 15, ink);
                circle(canvas, x, y, 10, fill);
            }
            // Cross.
            2 => {
                stroke(canvas, (x - 12, y - 12), (x + 12, y + 12), 5, ink);
                stroke(canvas, (x - 12, y + 12), (x + 12, y - 12), 5, ink);
            }
            // Square.
            _ => {
                canvas.fill_rect(x - 13, y - 13, 26, 26, ink);
                canvas.fill_rect(x - 8, y - 8, 16, 16, fill);
            }
        }
    }
}

/// The handheld's own face, as the generic gamepad has no other: laid out
/// as the buttons sit on the Brick.
fn brick_face(canvas: &mut Canvas, fonts: &mut Fonts, state: &hid::State) {
    use hid::button;
    let w = canvas.width();
    let (bx, by, bw, bh) = (64, 256, w - 128, 436);
    // Shoulders first, so the body overlaps their lower edge. On the
    // handheld L1/R1 sit at the outer corners and L2/R2 just inside them.
    shoulder(canvas, fonts, bx + 8, by - 64, 190, 64, "L1", state.pressed(button::L1), None);
    shoulder(canvas, fonts, bx + bw - 198, by - 64, 190, 64, "R1", state.pressed(button::R1), None);
    shoulder(canvas, fonts, bx + 210, by - 64, 170, 64, "L2", state.pressed(button::L2), Some(state.triggers[0]));
    shoulder(canvas, fonts, bx + bw - 380, by - 64, 170, 64, "R2", state.pressed(button::R2), Some(state.triggers[1]));
    round_rect(canvas, bx - 3, by - 3, bw + 6, bh + 6, 51, BODY_EDGE);
    round_rect(canvas, bx, by, bw, bh, 48, BODY);

    let mid_y = by + 158;
    dpad_sized(canvas, bx + 210, mid_y, 74, 112, state);

    // Diamond: X top, Y left, A right, B bottom, as printed on the handheld.
    let (fx, gap) = (bx + bw - 210, 92);
    face_button(canvas, fonts, fx, mid_y - gap, "X", state.pressed(button::NORTH));
    face_button(canvas, fonts, fx - gap, mid_y, "Y", state.pressed(button::WEST));
    face_button(canvas, fonts, fx + gap, mid_y, "A", state.pressed(button::EAST));
    face_button(canvas, fonts, fx, mid_y + gap, "B", state.pressed(button::SOUTH));

    // Sticks below the D-pad and the face buttons.
    let stick_y = by + bh - 74;
    let [lx, ly, rx, ry] = state.sticks;
    stick(canvas, bx + 210, stick_y, lx, ly, state.pressed(button::L3));
    stick(canvas, fx, stick_y, rx, ry, state.pressed(button::R3));

    let cx = bx + bw / 2;
    // MENU, SELECT, START from left to right, as on the handheld.
    pill(canvas, fonts, cx - 112, stick_y, 100, "MENU", state.pressed(button::HOME));
    pill(canvas, fonts, cx, stick_y, 100, "SELECT", state.pressed(button::SELECT));
    pill(canvas, fonts, cx + 112, stick_y, 100, "START", state.pressed(button::START));
}

fn draw(canvas: &mut Canvas, fonts: &mut Fonts, status: &Status, state: &hid::State, quitting: bool) {
    canvas.clear(BG);

    let (corner, colour, text): (String, Rgb, Vec<String>) = match status {
        Status::Starting => ("Starting".into(), DIM, vec!["Turning Bluetooth on…".into()]),
        Status::Failed(e) => ("Could not start".into(), ERROR, vec![e.clone(), "Press B or MENU to quit.".into()]),
        Status::Waiting { reconnecting, mode } => {
            let name = mode.bluetooth_name();
            let pair = match mode {
                Mode::Pc => format!("On a PC: Settings > Bluetooth > Add device > choose \"{name}\"."),
                Mode::Xbox | Mode::Ps4 => format!("On an iPad or PC: Settings > Bluetooth > choose \"{name}\"."),
            };
            let mut text = vec![pair];
            if let Some(host) = reconnecting {
                text.push(format!("Reconnecting to {host}…"));
            }
            (format!("{} · waiting", mode.label()), WARN, text)
        }
        Status::Connected { name, mode } => {
            let screen = if saver_on() {
                "Battery saver: the screen turns off after 8 seconds idle, and the CPU slows down."
            } else {
                "The screen dims after 20 seconds idle, to save battery."
            };
            (format!("{} · connected", mode.label()), OK, vec![format!("Connected to {name}."), screen.into()])
        }
    };
    header(canvas, fonts, NAME, &corner, colour);
    lines(canvas, fonts, &text);

    // A picture of the controller the host sees, lighting what is pressed.
    let mode = match status {
        Status::Waiting { mode, .. } | Status::Connected { mode, .. } => Some(*mode),
        _ => None,
    };
    match mode {
        Some(Mode::Xbox) => xbox_face(canvas, fonts, state),
        Some(Mode::Ps4) => ds4_face(canvas, fonts, state),
        _ => brick_face(canvas, fonts, state),
    }

    let credit = format!("{NAME} by {AUTHOR}");
    if quitting {
        footer(canvas, fonts, &[("MENU", "keep holding to quit…")], &credit);
    } else {
        footer(canvas, fonts, &[("SELECT", "mode"), ("MENU", "hold to quit")], &credit);
    }
}

/// The SELECT menu, over the whole screen: the modes, then the battery
/// saver.
fn draw_menu(canvas: &mut Canvas, fonts: &mut Fonts, menu: &Menu) {
    let w = canvas.width();
    canvas.clear(BG);
    header(canvas, fonts, "Mode", &format!("{NAME} {VERSION}"), DIM);

    let describe = |m: Mode| match m {
        Mode::Pc => ("PC", "a generic gamepad"),
        Mode::Xbox => ("Xbox", "Windows, iPad, Android"),
        Mode::Ps4 => ("PS4", "iPad, Android, PC"),
    };
    let (size, row_h) = (34.0, 56);
    for row in 0..MENU_ROWS {
        let baseline = CONTENT_TOP + 8 + row as i32 * row_h;
        if row == menu.cursor {
            canvas.fill_rect(LEFT - 12, baseline - row_h + 14, w - LEFT * 2 + 24, row_h, CURSOR);
        }
        let (name, about, on) = match Mode::ALL.get(row) {
            Some(&m) => {
                let (name, about) = describe(m);
                (name, about, (m == menu.current).then_some("in use"))
            }
            None => ("Battery saver", "screen off sooner, slower CPU", Some(if menu.saver { "on" } else { "off" })),
        };
        let x = fonts.draw(canvas, name, LEFT, baseline, size, FG);
        fonts.draw(canvas, about, x + 16, baseline, size * 0.7, DIM);
        if let Some(on) = on {
            let ow = fonts.measure(on, size * 0.8);
            fonts.draw(canvas, on, w - LEFT - ow, baseline, size * 0.8, if on == "off" { DIM } else { ACCENT });
        }
    }
    footer(canvas, fonts, &[("A", "choose"), ("B", "close")], &format!("by {AUTHOR} · {HOME_PAGE}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `PADPOD_PREVIEW=dir cargo test preview` writes each screen as a PNG,
    /// for checking the layout without the device.
    #[test]
    fn preview_every_screen() {
        let Ok(dir) = std::env::var("PADPOD_PREVIEW") else { return };
        let mut fonts = Fonts::load(&[]);
        let mut pressed = hid::State::default();
        pressed.set_button(hid::button::SOUTH, true);
        pressed.set_button(hid::button::R1, true);
        pressed.hat = 1;
        pressed.triggers[0] = 120;
        pressed.sticks = [128, 128, 255, 60];
        pressed.set_button(hid::button::L3, true);
        let screens = [
            ("starting", Status::Starting, hid::State::default()),
            ("failed", Status::Failed("Bluetooth is off. Turn it on in the firmware's settings and open Padpod again.".into()), hid::State::default()),
            ("waiting", Status::Waiting { reconnecting: Some("Pixel 9".into()), mode: Mode::Ps4 }, hid::State::default()),
            ("connected", Status::Connected { name: "my-pc".into(), mode: Mode::Pc }, pressed),
            ("xbox", Status::Connected { name: "Windows PC".into(), mode: Mode::Xbox }, pressed),
            ("ps4", Status::Connected { name: "iPad".into(), mode: Mode::Ps4 }, pressed),
            ("xbox-rest", Status::Waiting { reconnecting: None, mode: Mode::Xbox }, hid::State::default()),
        ];
        let mut canvas = Canvas::in_memory(1024, 768);
        draw(&mut canvas, &mut fonts, &Status::Waiting { reconnecting: None, mode: Mode::Pc }, &hid::State::default(), false);
        draw_menu(&mut canvas, &mut fonts, &Menu { cursor: 3, current: Mode::Pc, saver: true });
        png::write_png(&format!("{dir}/menu.png"), 1024, 768, &canvas.snapshot_rgb()).unwrap();
        for (name, status, state) in screens {
            let mut canvas = Canvas::in_memory(1024, 768);
            draw(&mut canvas, &mut fonts, &status, &state, false);
            png::write_png(&format!("{dir}/{name}.png"), 1024, 768, &canvas.snapshot_rgb()).unwrap();
        }
    }

    #[test]
    fn the_menu_moves_wraps_and_answers_a_and_b() {
        let rest = hid::State::default();
        let mut menu = Menu { cursor: 0, current: Mode::Pc, saver: false };
        let mut down = rest;
        down.hat = 4;
        assert_eq!(menu_press(&mut menu, &rest, &down), MenuAction::Stay);
        assert_eq!(menu.cursor, 1);
        // Held, not pressed again: no move.
        menu_press(&mut menu, &down, &down);
        assert_eq!(menu.cursor, 1);
        let mut a = rest;
        a.set_button(hid::button::EAST, true);
        assert_eq!(menu_press(&mut menu, &rest, &a), MenuAction::Switch(Mode::Xbox));
        menu.cursor = 0;
        assert_eq!(menu_press(&mut menu, &rest, &a), MenuAction::Close, "the mode already on");
        menu.cursor = MENU_ROWS - 1;
        assert_eq!(menu_press(&mut menu, &rest, &a), MenuAction::ToggleSaver);
        menu_press(&mut menu, &rest, &down);
        assert_eq!(menu.cursor, 0, "wraps");
        let mut b = rest;
        b.set_button(hid::button::SOUTH, true);
        assert_eq!(menu_press(&mut menu, &rest, &b), MenuAction::Close);
    }

    #[test]
    fn a_hat_direction_lights_its_arrow_and_the_diagonals_beside_it() {
        let mut s = hid::State::default();
        s.hat = 1; // up-right
        assert!(lit(Control::Hat(0), &s));
        assert!(lit(Control::Hat(2), &s));
        assert!(!lit(Control::Hat(4), &s));
        assert!(!lit(Control::Hat(6), &s));
        s.hat = hid::HAT_CENTRED;
        assert!(!lit(Control::Hat(0), &s));
    }
}
