use core::fmt;

use crate::catalogue::ProjectId;
use crate::placement::PlacementName;

pub const FRESH_SECS: u64 = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PodPhase {
    Pending,
    Running,
    Succeeded,
    Failed,
    Unknown,
}

impl PodPhase {
    pub fn of_value(value: f64) -> Option<Self> {
        let distance = |code: f64| (value - code).abs() < 0.5;
        [
            (1.0, Self::Pending),
            (2.0, Self::Running),
            (3.0, Self::Succeeded),
            (4.0, Self::Failed),
            (5.0, Self::Unknown),
        ]
        .into_iter()
        .find_map(|(code, phase)| distance(code).then_some(phase))
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Running => "Running",
            Self::Succeeded => "Succeeded",
            Self::Failed => "Failed",
            Self::Unknown => "Unknown",
        }
    }

    pub fn is_healthy(self) -> bool {
        matches!(self, Self::Running | Self::Succeeded)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PodReason {
    Restarting,
    NotReady,
    NodeNotReady,
    Phase(PodPhase),
}

impl PodReason {
    pub fn name(self) -> &'static str {
        match self {
            Self::Restarting => "restarting",
            Self::NotReady => "not ready",
            Self::NodeNotReady => "node not ready",
            Self::Phase(PodPhase::Pending) => "pending",
            Self::Phase(PodPhase::Running) => "running",
            Self::Phase(PodPhase::Succeeded) => "succeeded",
            Self::Phase(PodPhase::Failed) => "failed",
            Self::Phase(PodPhase::Unknown) => "unknown",
        }
    }
}

impl fmt::Display for PodReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClusterFilter {
    pub cluster: Option<PlacementName>,
    pub namespace: Option<PlacementName>,
    pub project: Option<ProjectId>,
}
