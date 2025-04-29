//! Utilities to help with debugging, should be removed before production release.

use std::time::Instant;

pub struct TimingHelper(Instant);

impl Default for TimingHelper {
    fn default() -> Self {
        Self::new()
    }
}

impl TimingHelper {
    pub fn new() -> Self {
        TimingHelper(Instant::now())
    }

    pub fn report(&mut self, title: &str) {
        let now = Instant::now();
        println!("{title}: {:?}", now.duration_since(self.0));
        self.0 = now;
    }
}
