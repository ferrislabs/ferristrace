#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Window {
    Quarter,
    Hour,
    Day,
    Week,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("window must be one of 15m, 1h, 24h or 7d, not {0:?}")]
pub struct BadWindow(pub String);

impl Window {
    pub fn parse(spelling: Option<&str>) -> Result<Self, BadWindow> {
        let Some(typed) = spelling.map(str::trim).filter(|typed| !typed.is_empty()) else {
            return Ok(Self::Hour);
        };
        match typed.to_ascii_lowercase().as_str() {
            "15m" => Ok(Self::Quarter),
            "1h" => Ok(Self::Hour),
            "24h" => Ok(Self::Day),
            "7d" => Ok(Self::Week),
            _ => Err(BadWindow(typed.to_owned())),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Quarter => "15m",
            Self::Hour => "1h",
            Self::Day => "24h",
            Self::Week => "7d",
        }
    }

    pub fn millis(self) -> u64 {
        match self {
            Self::Quarter => 15 * 60 * 1000,
            Self::Hour => 60 * 60 * 1000,
            Self::Day => 24 * 60 * 60 * 1000,
            Self::Week => 7 * 24 * 60 * 60 * 1000,
        }
    }

    pub fn bounds(self, now_ms: u64) -> (u64, u64) {
        (now_ms.saturating_sub(self.millis()), now_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_given_means_an_hour() {
        assert_eq!(Window::parse(None), Ok(Window::Hour));
        assert_eq!(Window::parse(Some("  ")), Ok(Window::Hour));
    }

    #[test]
    fn the_four_spellings_are_understood() {
        assert_eq!(Window::parse(Some("15m")), Ok(Window::Quarter));
        assert_eq!(Window::parse(Some("1h")), Ok(Window::Hour));
        assert_eq!(Window::parse(Some("24h")), Ok(Window::Day));
        assert_eq!(Window::parse(Some("7d")), Ok(Window::Week));
        assert_eq!(Window::parse(Some(" 7D ")), Ok(Window::Week));
    }

    #[test]
    fn anything_else_is_refused_with_the_accepted_values() {
        let refused = Window::parse(Some("3w")).unwrap_err();
        assert_eq!(refused, BadWindow("3w".to_owned()));
        let message = refused.to_string();
        for accepted in ["15m", "1h", "24h", "7d"] {
            assert!(message.contains(accepted), "{message}");
        }
    }

    #[test]
    fn bounds_end_now_and_start_one_window_earlier() {
        let now = 1_700_000_000_000;
        assert_eq!(Window::Quarter.bounds(now), (now - 900_000, now));
        assert_eq!(Window::Week.bounds(now), (now - 604_800_000, now));
        assert_eq!(Window::Hour.bounds(1_000), (0, 1_000));
    }

    #[test]
    fn names_round_trip() {
        for window in [Window::Quarter, Window::Hour, Window::Day, Window::Week] {
            assert_eq!(Window::parse(Some(window.name())), Ok(window));
        }
    }
}
