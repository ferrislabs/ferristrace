use core::fmt;

use uuid::Uuid;

use super::errors::{BadCallQuery, BadPrice};
use crate::catalogue::ProjectId;
use crate::explore::Window;
use crate::telemetry::TraceId;

pub const MAX_PROVIDER_BYTES: usize = 64;
pub const MAX_MODEL_BYTES: usize = 128;
pub const MAX_RATE: u64 = 1_000_000_000_000;
pub const DEFAULT_CALL_LIMIT: u32 = 100;
pub const MAX_CALL_LIMIT: u32 = 500;
pub const MODEL_LIMIT: u32 = 500;

const PICODOLLARS_PER_DOLLAR: f64 = 1_000_000_000_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PriceId(Uuid);

impl PriceId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }

    pub fn fresh() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl fmt::Display for PriceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Provider(Box<str>);

impl Provider {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is(&self, spelling: &str) -> bool {
        self.0.eq_ignore_ascii_case(spelling.trim())
    }
}

impl TryFrom<&str> for Provider {
    type Error = BadPrice;

    fn try_from(spelling: &str) -> Result<Self, BadPrice> {
        let trimmed = spelling.trim();
        if trimmed.is_empty()
            || trimmed.len() > MAX_PROVIDER_BYTES
            || trimmed.contains(char::is_whitespace)
        {
            return Err(BadPrice::Provider {
                max: MAX_PROVIDER_BYTES,
            });
        }
        Ok(Self(trimmed.to_ascii_lowercase().into()))
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModelPattern(Box<str>);

impl ModelPattern {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn prefix(&self) -> Option<&str> {
        self.0.strip_suffix('*')
    }

    pub fn matches(&self, model: &str) -> Option<(bool, usize)> {
        match self.prefix() {
            Some(prefix) => model.starts_with(prefix).then_some((false, prefix.len())),
            None => (*self.0 == *model).then_some((true, self.0.len())),
        }
    }
}

impl TryFrom<&str> for ModelPattern {
    type Error = BadPrice;

    fn try_from(spelling: &str) -> Result<Self, BadPrice> {
        let trimmed = spelling.trim();
        if trimmed.is_empty() || trimmed.len() > MAX_MODEL_BYTES {
            return Err(BadPrice::Model {
                max: MAX_MODEL_BYTES,
            });
        }
        let body = trimmed.strip_suffix('*').unwrap_or(trimmed);
        if body.contains(['*', ' ', '\t', '\n']) {
            return Err(BadPrice::Pattern);
        }
        Ok(Self(trimmed.into()))
    }
}

impl fmt::Display for ModelPattern {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rates {
    pub input: u64,
    pub output: u64,
    pub cached_input: Option<u64>,
}

impl Rates {
    pub fn new(input: u64, output: u64, cached_input: Option<u64>) -> Result<Self, BadPrice> {
        let all = [Some(input), Some(output), cached_input];
        if all.into_iter().flatten().any(|rate| rate > MAX_RATE) {
            return Err(BadPrice::Rate { max: MAX_RATE });
        }
        Ok(Self {
            input,
            output,
            cached_input,
        })
    }

    pub fn cost(&self, input_tokens: u64, output_tokens: u64, cached_tokens: u64) -> Cost {
        let cached = cached_tokens.min(input_tokens);
        let fresh = input_tokens - cached;
        let cached_rate = self.cached_input.unwrap_or(self.input);
        Cost(
            u128::from(fresh) * u128::from(self.input)
                + u128::from(cached) * u128::from(cached_rate)
                + u128::from(output_tokens) * u128::from(self.output),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceDraft {
    pub provider: Provider,
    pub model: ModelPattern,
    pub rates: Rates,
    pub effective_from_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Cost(u128);

impl Cost {
    pub fn picodollars(self) -> u128 {
        self.0
    }

    pub fn dollars(self) -> f64 {
        self.0 as f64 / PICODOLLARS_PER_DOLLAR
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CallCursor {
    pub time_unix_nanos: u64,
    pub tiebreak: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallQuery {
    pub window: Window,
    pub project: Option<ProjectId>,
    pub feature: Option<Box<str>>,
    pub model: Option<Box<str>>,
    pub customer: Option<Box<str>>,
    pub user: Option<Box<str>>,
    pub prompt_version: Option<Box<str>>,
    pub provider: Option<Box<str>>,
    pub trace: Option<TraceId>,
    pub before: Option<CallCursor>,
    limit: u32,
}

impl CallQuery {
    pub fn over(window: Window) -> Self {
        Self {
            window,
            project: None,
            feature: None,
            model: None,
            customer: None,
            user: None,
            prompt_version: None,
            provider: None,
            trace: None,
            before: None,
            limit: DEFAULT_CALL_LIMIT,
        }
    }

    pub fn at_most(mut self, asked: u32) -> Result<Self, BadCallQuery> {
        if !(1..=MAX_CALL_LIMIT).contains(&asked) {
            return Err(BadCallQuery::Limit {
                max: MAX_CALL_LIMIT,
            });
        }
        self.limit = asked;
        Ok(self)
    }

    pub fn limit(&self) -> u32 {
        self.limit
    }
}
