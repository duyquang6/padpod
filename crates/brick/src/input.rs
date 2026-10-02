// Copyright 2026 ligt (https://github.com/duyquang6/padpod)
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The pad, read straight from its evdev node (as Truepod does).

use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Read};
use std::os::unix::io::AsRawFd;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    X,
    Y,
    L1,
    R1,
    L2,
    R2,
    Start,
    Select,
    Menu,
}

/// Key codes from spruce/scripts/platform/BrickPro.cfg.
pub fn map_key(code: u16) -> Option<Button> {
    Some(match code {
        305 => Button::A,
        304 => Button::B,
        308 => Button::X,
        307 => Button::Y,
        310 => Button::L1,
        311 => Button::R1,
        315 => Button::Start,
        314 => Button::Select,
        316 => Button::Menu,
        _ => return None,
    })
}

const EV_KEY: u16 = 1;
const EV_ABS: u16 = 3;
const ABS_Z: u16 = 2;
const ABS_RZ: u16 = 5;
const ABS_HAT0X: u16 = 16;
const ABS_HAT0Y: u16 = 17;

/// The D-pad is a hat axis and the triggers report pressure (255 on push, 0 on
/// release). A zero is a release and must produce nothing.
pub fn map_abs(code: u16, value: i32) -> Option<Button> {
    match code {
        ABS_HAT0X if value < 0 => Some(Button::Left),
        ABS_HAT0X if value > 0 => Some(Button::Right),
        ABS_HAT0Y if value < 0 => Some(Button::Up),
        ABS_HAT0Y if value > 0 => Some(Button::Down),
        ABS_Z if value > 0 => Some(Button::L2),
        ABS_RZ if value > 0 => Some(Button::R2),
        _ => None,
    }
}

/// How long a button is held before it starts repeating, and how fast.
const REPEAT_DELAY: Duration = Duration::from_millis(380);
const REPEAT_EVERY: Duration = Duration::from_millis(110);

fn repeats(button: Button) -> bool {
    use Button::*;
    matches!(button, Up | Down | Left | Right | L1 | R1)
}

/// struct input_event on 64-bit Linux.
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct InputEvent {
    tv_sec: i64,
    tv_usec: i64,
    kind: u16,
    code: u16,
    value: i32,
}

/// The pad's input node, found by name: it is usually `event3`, but a USB
/// device present at boot can take that number first.
pub fn pad_path() -> String {
    std::fs::read_to_string("/proc/bus/input/devices")
        .ok()
        .and_then(|text| parse_pad(&text))
        .map(|event| format!("/dev/input/{event}"))
        .unwrap_or_else(|| "/dev/input/event3".to_string())
}

pub fn parse_pad(devices: &str) -> Option<String> {
    devices
        .split("\n\n")
        .find(|block| block.lines().any(|l| l == "N: Name=\"TRIMUI Player1\""))?
        .lines()
        .find_map(|l| l.strip_prefix("H: Handlers="))?
        .split_whitespace()
        .find(|h| h.starts_with("event"))
        .map(str::to_string)
}

pub struct Pad {
    file: File,
    /// The repeating button currently held, and when it fires next.
    held: Option<(Button, Instant)>,
}

impl Pad {
    /// Opened non-exclusively on purpose: spruce's own watchdogs read the same
    /// node for the brightness and sleep hotkeys, and a grab would break them.
    pub fn open(path: &str) -> io::Result<Self> {
        Ok(Self { file: File::open(path)?, held: None })
    }

    /// Wait up to `timeout` for a press, or for a held button's next repeat.
    pub fn poll(&mut self, timeout: Duration) -> io::Result<Option<Button>> {
        let now = Instant::now();
        let wait = match self.held {
            Some((_, due)) => timeout.min(due.saturating_duration_since(now)),
            None => timeout,
        };
        let mut pfd = libc::pollfd { fd: self.file.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        let rc = unsafe { libc::poll(&mut pfd, 1, wait.as_millis() as i32) };
        if rc < 0 {
            let err = io::Error::last_os_error();
            // A resume from suspend interrupts the poll; that is a quiet tick.
            return if err.kind() == io::ErrorKind::Interrupted { Ok(None) } else { Err(err) };
        }
        if rc == 0 {
            if let Some((button, due)) = self.held {
                if Instant::now() >= due {
                    self.held = Some((button, due + REPEAT_EVERY));
                    return Ok(Some(button));
                }
            }
            return Ok(None);
        }

        let mut buf = [0u8; std::mem::size_of::<InputEvent>()];
        if let Err(e) = self.file.read_exact(&mut buf) {
            return if e.kind() == io::ErrorKind::Interrupted { Ok(None) } else { Err(e) };
        }
        let ev: InputEvent = unsafe { std::ptr::read_unaligned(buf.as_ptr() as *const InputEvent) };

        let pressed = match ev.kind {
            EV_KEY if ev.value == 1 => map_key(ev.code),
            EV_ABS => map_abs(ev.code, ev.value),
            _ => None,
        };
        let released = match ev.kind {
            EV_KEY if ev.value == 0 => true,
            EV_ABS if matches!(ev.code, ABS_HAT0X | ABS_HAT0Y) && ev.value == 0 => true,
            _ => false,
        };
        if let Some(button) = pressed {
            self.held = repeats(button).then(|| (button, Instant::now() + REPEAT_DELAY));
        } else if released {
            self.held = None;
        }
        Ok(pressed)
    }
}

/// One step of a scripted session, for exercising the app with no one at the
/// buttons - on the host, or on the device over ssh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Press(Button),
    /// Wait until the app's background work has finished (or this many ms pass).
    Settle(u64),
    /// Wait this many ms regardless - to let an episode play.
    Wait(u64),
    /// Save what is on screen as a PNG.
    Shot(String),
}

/// Parse `down,a,settle,wait:5000,shot:/tmp/x.png,right*3` into steps.
/// `settle_ms` is how long `settle` may wait, which is the app's to say.
pub fn parse_script(script: &str, settle_ms: u64) -> Result<VecDeque<Step>, String> {
    let mut steps = VecDeque::new();
    for item in script.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(path) = item.strip_prefix("shot:") {
            steps.push_back(Step::Shot(path.to_string()));
            continue;
        }
        if item == "settle" {
            steps.push_back(Step::Settle(settle_ms));
            continue;
        }
        if let Some(ms) = item.strip_prefix("wait:") {
            steps.push_back(Step::Wait(ms.parse().map_err(|_| format!("bad wait in '{item}'"))?));
            continue;
        }
        let (name, count) = match item.split_once('*') {
            Some((name, n)) => (name, n.parse::<usize>().map_err(|_| format!("bad count in '{item}'"))?),
            None => (item, 1),
        };
        let button = match name {
            "up" => Button::Up,
            "down" => Button::Down,
            "left" => Button::Left,
            "right" => Button::Right,
            "a" => Button::A,
            "b" => Button::B,
            "x" => Button::X,
            "y" => Button::Y,
            "l1" => Button::L1,
            "r1" => Button::R1,
            "l2" => Button::L2,
            "r2" => Button::R2,
            "start" => Button::Start,
            "select" => Button::Select,
            "menu" => Button::Menu,
            other => return Err(format!("unknown script step '{other}'")),
        };
        steps.extend(std::iter::repeat(Step::Press(button)).take(count));
    }
    Ok(steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_brickpro_key_codes() {
        assert_eq!(map_key(305), Some(Button::A));
        assert_eq!(map_key(304), Some(Button::B));
        assert_eq!(map_key(316), Some(Button::Menu));
        assert_eq!(map_key(9999), None);
    }

    #[test]
    fn the_dpad_is_a_hat_and_a_release_is_not_a_press() {
        assert_eq!(map_abs(ABS_HAT0X, -1), Some(Button::Left));
        assert_eq!(map_abs(ABS_HAT0X, 1), Some(Button::Right));
        assert_eq!(map_abs(ABS_HAT0Y, -1), Some(Button::Up));
        assert_eq!(map_abs(ABS_HAT0Y, 1), Some(Button::Down));
        assert_eq!(map_abs(ABS_HAT0X, 0), None);
        assert_eq!(map_abs(ABS_Z, 255), Some(Button::L2));
        assert_eq!(map_abs(ABS_RZ, 0), None);
    }

    #[test]
    fn finds_the_pad_by_name() {
        let devices = "N: Name=\"audiocodec sunxi Audio Jack\"\nH: Handlers=kbd event2 \n\n\
                       N: Name=\"TRIMUI Player1\"\nH: Handlers=kbd js0 event4 \n";
        assert_eq!(parse_pad(devices).as_deref(), Some("event4"));
        assert_eq!(parse_pad("N: Name=\"other\"\nH: Handlers=kbd event3\n"), None);
    }

    #[test]
    fn input_event_matches_the_kernel_layout() {
        assert_eq!(std::mem::size_of::<InputEvent>(), 24);
    }

    #[test]
    fn a_script_expands_repeats_and_keeps_shots_in_order() {
        let steps = parse_script("down*2, a, settle, wait:1500, shot:/tmp/x.png", 120_000).unwrap();
        assert_eq!(
            Vec::from(steps),
            vec![
                Step::Press(Button::Down),
                Step::Press(Button::Down),
                Step::Press(Button::A),
                Step::Settle(120_000),
                Step::Wait(1500),
                Step::Shot("/tmp/x.png".into()),
            ]
        );
        assert!(parse_script("jump", 1).is_err());
        assert!(parse_script("a*x", 1).is_err());
        assert!(parse_script("wait:soon", 1).is_err());
    }
}
