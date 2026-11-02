use crate::issue::Counted;
use crate::telemetry::TenantId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub service: Box<str>,
    pub version: Box<str>,
    pub environments: Vec<Box<str>>,
    pub first_seen_unix_nanos: u64,
    pub last_seen_unix_nanos: u64,
    pub requests: u64,
    pub errors: u64,
    pub new_issues: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Traffic {
    pub requests: u64,
    pub errors: u64,
}

impl Traffic {
    #[allow(clippy::cast_precision_loss)]
    pub fn error_rate(self) -> Option<f64> {
        (self.requests > 0).then(|| self.errors as f64 / self.requests as f64)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReleaseDetail {
    pub release: Release,
    pub before: Traffic,
    pub after: Traffic,
    pub introduced: Vec<Counted>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deploy {
    pub service: Box<str>,
    pub version: Box<str>,
    pub environment: Box<str>,
    pub at_unix_nanos: u64,
}

impl Deploy {
    pub fn at_ms(&self) -> u64 {
        self.at_unix_nanos / 1_000_000
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deployed {
    pub org: TenantId,
    pub deploy: Deploy,
}
