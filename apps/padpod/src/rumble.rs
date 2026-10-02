//! The handheld's rumble motor, driven by what the host asks for.
//!
//! On the Brick Pro the motor hangs off GPIO 227 (PH3): `1` spins it, `0`
//! stops it, and nothing in between. spruce exports the pin at boot; this only
//! writes its value, as Truepod does.
//!
//! A host asks for a strength (0-255), so the motor is switched on and off
//! within a 20 ms cycle, on for that share of it. A coin motor cannot follow
//! 50 Hz, so the cycle reads as a weaker or stronger buzz rather than as
//! pulses.

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

const GPIO: &str = "/sys/class/gpio/gpio227/value";
const CYCLE: Duration = Duration::from_millis(20);
/// Below this the motor would only twitch; it stays off.
const LEAST: u8 = 12;

pub struct Rumble {
    level: Arc<AtomicU8>,
    running: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// How long the motor is on in each cycle for a requested strength.
pub fn on_time(level: u8) -> Duration {
    if level < LEAST {
        Duration::ZERO
    } else {
        CYCLE * level as u32 / 255
    }
}

impl Rumble {
    /// None when the pin is not there (another device, or spruce never
    /// exported it): the gamepad works the same, without rumble.
    pub fn open() -> Option<Self> {
        let mut file = OpenOptions::new().write(true).open(GPIO).ok()?;
        let _ = file.write_all(b"0");
        let level = Arc::new(AtomicU8::new(0));
        let running = Arc::new(AtomicBool::new(true));
        let (l, r) = (Arc::clone(&level), Arc::clone(&running));
        let thread = std::thread::spawn(move || {
            let mut on = false;
            let mut set = |want: bool| {
                if want != on {
                    let _ = file.write_all(if want { b"1" } else { b"0" });
                    on = want;
                }
            };
            while r.load(Ordering::Relaxed) {
                let lit = on_time(l.load(Ordering::Relaxed));
                if lit.is_zero() {
                    set(false);
                    std::thread::sleep(CYCLE);
                    continue;
                }
                set(true);
                std::thread::sleep(lit);
                if lit < CYCLE {
                    set(false);
                    std::thread::sleep(CYCLE - lit);
                }
            }
            set(false);
        });
        Some(Self { level, running, thread: Some(thread) })
    }

    pub fn set(&self, level: u8) {
        self.level.store(level, Ordering::Relaxed);
    }
}

impl Drop for Rumble {
    /// A motor left on spins until something else writes the pin.
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strength_sets_the_share_of_each_cycle_the_motor_is_on() {
        assert_eq!(on_time(0), Duration::ZERO);
        assert_eq!(on_time(LEAST - 1), Duration::ZERO, "too weak to feel");
        assert_eq!(on_time(255), CYCLE);
        assert_eq!(on_time(128), CYCLE * 128 / 255);
    }
}
