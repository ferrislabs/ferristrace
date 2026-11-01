#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnixSeconds(u64);

impl UnixSeconds {
    pub fn new(seconds: u64) -> Self {
        Self(seconds)
    }

    pub fn get(self) -> u64 {
        self.0
    }

    pub fn saturating_add_secs(self, seconds: u64) -> Self {
        Self(self.0.saturating_add(seconds))
    }
}

pub trait Clock {
    fn now(&self) -> UnixSeconds;
}

impl<T: Clock> Clock for &T {
    fn now(&self) -> UnixSeconds {
        (*self).now()
    }
}

impl<T: Clock> Clock for std::sync::Arc<T> {
    fn now(&self) -> UnixSeconds {
        self.as_ref().now()
    }
}

#[cfg(test)]
mod tests {
    use super::UnixSeconds;

    #[test]
    fn adding_seconds_saturates_rather_than_wrapping() {
        let late = UnixSeconds::new(u64::MAX);

        assert_eq!(late.saturating_add_secs(10), late);
    }

    #[test]
    fn adding_seconds_moves_the_instant_forward() {
        assert_eq!(
            UnixSeconds::new(100).saturating_add_secs(23),
            UnixSeconds::new(123)
        );
    }
}
