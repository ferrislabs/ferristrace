use crate::catalogue::ProjectId;
use crate::placement::PlacementName;

pub const SILENT_SECS: u64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ResourceKind {
    Cluster,
    Host,
    Container,
}

impl ResourceKind {
    pub const ALL: &'static [Self] = &[Self::Cluster, Self::Host, Self::Container];

    pub fn name(self) -> &'static str {
        match self {
            Self::Cluster => "cluster",
            Self::Host => "host",
            Self::Container => "container",
        }
    }

    pub fn parse(spelling: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|kind| kind.name() == spelling)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceStatus {
    Active,
    Silent,
}

impl ResourceStatus {
    pub fn parse(spelling: &str) -> Option<Self> {
        match spelling {
            "active" => Some(Self::Active),
            "silent" => Some(Self::Silent),
            _ => None,
        }
    }

    pub fn holds(self, silent: bool) -> bool {
        silent == matches!(self, Self::Silent)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InfraFilter {
    pub kind: Option<ResourceKind>,
    pub cluster: Option<PlacementName>,
    pub environment: Option<PlacementName>,
    pub provider: Option<PlacementName>,
    pub region: Option<PlacementName>,
    pub status: Option<ResourceStatus>,
    pub project: Option<ProjectId>,
}
