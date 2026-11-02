use std::time::{SystemTime, UNIX_EPOCH};

use ferristrace_domain::{Clock, UnixSeconds};

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> UnixSeconds {
        UnixSeconds::new(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| elapsed.as_secs())
                .unwrap_or_default(),
        )
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FixedClock(u64);

impl FixedClock {
    pub fn at(seconds: u64) -> Self {
        Self(seconds)
    }
}

impl Clock for FixedClock {
    fn now(&self) -> UnixSeconds {
        UnixSeconds::new(self.0)
    }
}

#[cfg(test)]
mod tests {
    use ferristrace_domain::{Clock, UnixSeconds};

    use super::{FixedClock, SystemClock};

    #[test]
    fn a_fixed_clock_does_not_move() {
        let clock = FixedClock::at(1_700_000_000);

        assert_eq!(clock.now(), UnixSeconds::new(1_700_000_000));
        assert_eq!(clock.now(), clock.now());
    }

    #[test]
    fn the_system_clock_is_past_the_epoch() {
        assert!(SystemClock.now().get() > 1_700_000_000);
    }
}
