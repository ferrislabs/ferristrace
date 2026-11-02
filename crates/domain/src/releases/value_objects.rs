use crate::catalogue::ProjectId;
use crate::explore::Window;

pub const RELEASE_LIMIT: u32 = 500;
pub const DEPLOY_FEED_LIMIT: u32 = 5_000;
pub const DEPLOY_WATCH_TICK_SECS: u64 = 60;
pub const LATE_SECS: u64 = 300;
pub const TRAFFIC_HOUR_NANOS: u64 = 3_600_000_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseFilter {
    pub window: Window,
    pub service: Option<Box<str>>,
    pub project: Option<ProjectId>,
}

impl ReleaseFilter {
    pub fn over(window: Window) -> Self {
        Self {
            window,
            service: None,
            project: None,
        }
    }
}
