//! One-shot and repeating arms polled by the Bluetooth task.
//!
//! These replace the ESP-IDF timers. Nothing here runs in a callback: the
//! link task calls [`Deadline::poll`] and then does the work itself.

use crate::clock;

pub struct Deadline {
    at: i64,
    period: u64,
}

impl Deadline {
    pub const fn new() -> Self {
        Self { at: 0, period: 0 }
    }

    pub fn stop(&mut self) {
        self.at = 0;
        self.period = 0;
    }

    pub fn armed(&self) -> bool {
        self.at != 0
    }

    pub fn start_once(&mut self, delay_us: u64) {
        self.period = 0;
        let delay = (delay_us as i64).max(1);
        self.at = clock::now_us().saturating_add(delay);
    }

    pub fn start_periodic(&mut self, period_us: u64) {
        self.period = period_us.max(1);
        self.at = clock::now_us().saturating_add(self.period as i64);
    }

    /// Returns true once, when the arm is due. A repeating arm is then set
    /// one period ahead of now.
    pub fn poll(&mut self) -> bool {
        if self.at == 0 {
            return false;
        }
        let now = clock::now_us();
        if now < self.at {
            return false;
        }
        if self.period == 0 {
            self.at = 0;
        } else {
            self.at = now.saturating_add(self.period as i64);
        }
        true
    }
}
