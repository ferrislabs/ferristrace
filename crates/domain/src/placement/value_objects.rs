use core::fmt;
use std::collections::BTreeMap;

use super::errors::BadPlacement;

pub const MAX_PLACEMENT_BYTES: usize = 253;
pub const MAX_PLACEMENTS: usize = 500;
pub const CLUSTER_KEY: &str = "k8s.cluster.name";
pub const NAMESPACE_KEY: &str = "k8s.namespace.name";
pub const POD_KEY: &str = "k8s.pod.name";
pub const NODE_KEY: &str = "k8s.node.name";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WorkloadKind {
    Deployment,
    StatefulSet,
    DaemonSet,
    CronJob,
    Job,
}

impl WorkloadKind {
    pub const ALL: &'static [Self] = &[
        Self::Deployment,
        Self::StatefulSet,
        Self::DaemonSet,
        Self::CronJob,
        Self::Job,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Deployment => "deployment",
            Self::StatefulSet => "statefulset",
            Self::DaemonSet => "daemonset",
            Self::CronJob => "cronjob",
            Self::Job => "job",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Deployment => "k8s.deployment.name",
            Self::StatefulSet => "k8s.statefulset.name",
            Self::DaemonSet => "k8s.daemonset.name",
            Self::CronJob => "k8s.cronjob.name",
            Self::Job => "k8s.job.name",
        }
    }

    pub fn parse(spelling: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|kind| kind.name().eq_ignore_ascii_case(spelling.trim()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlacementName(Box<str>);

impl PlacementName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for PlacementName {
    type Error = BadPlacement;

    fn try_from(given: &str) -> Result<Self, BadPlacement> {
        let kept = given.trim();
        if kept.is_empty() {
            return Err(BadPlacement::Blank);
        }
        if kept.len() > MAX_PLACEMENT_BYTES {
            return Err(BadPlacement::TooLong {
                max: MAX_PLACEMENT_BYTES,
            });
        }
        let edges = |byte: Option<&u8>| byte.is_some_and(u8::is_ascii_alphanumeric);
        let inner = |byte: &u8| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_');
        let bytes = kept.as_bytes();
        if !(edges(bytes.first()) && edges(bytes.last()) && bytes.iter().all(inner)) {
            return Err(BadPlacement::Characters);
        }
        Ok(Self(kept.into()))
    }
}

impl fmt::Display for PlacementName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Workload {
    kind: Option<WorkloadKind>,
    name: PlacementName,
}

impl Workload {
    pub fn kind(&self) -> Option<WorkloadKind> {
        self.kind
    }

    pub fn name(&self) -> &PlacementName {
        &self.name
    }

    pub fn owner_of(attributes: &BTreeMap<Box<str>, Box<str>>) -> Option<(WorkloadKind, &str)> {
        WorkloadKind::ALL.iter().find_map(|kind| {
            attributes
                .get(kind.key())
                .filter(|name| !name.is_empty())
                .map(|name| (*kind, &**name))
        })
    }

    pub fn matches(&self, attributes: &BTreeMap<Box<str>, Box<str>>) -> bool {
        match self.kind {
            Some(kind) => {
                attributes.get(kind.key()).map(|name| &**name) == Some(self.name.as_str())
            }
            None => Self::owner_of(attributes).map(|(_, name)| name) == Some(self.name.as_str()),
        }
    }
}

impl TryFrom<&str> for Workload {
    type Error = BadPlacement;

    fn try_from(given: &str) -> Result<Self, BadPlacement> {
        match given.split_once(':') {
            Some((kind, name)) => Ok(Self {
                kind: Some(WorkloadKind::parse(kind).ok_or(BadPlacement::Kind)?),
                name: PlacementName::try_from(name)?,
            }),
            None => Ok(Self {
                kind: None,
                name: PlacementName::try_from(given)?,
            }),
        }
    }
}

impl fmt::Display for Workload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            Some(kind) => write!(formatter, "{}:{}", kind.name(), self.name),
            None => formatter.write_str(self.name.as_str()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Placement {
    pub cluster: Option<PlacementName>,
    pub namespace: Option<PlacementName>,
    pub workload: Option<Workload>,
    pub pod: Option<PlacementName>,
    pub node: Option<PlacementName>,
}

impl Placement {
    pub fn is_empty(&self) -> bool {
        self.cluster.is_none()
            && self.namespace.is_none()
            && self.workload.is_none()
            && self.pod.is_none()
            && self.node.is_none()
    }

    pub fn exact(&self) -> impl Iterator<Item = (&'static str, &PlacementName)> {
        [
            (CLUSTER_KEY, self.cluster.as_ref()),
            (NAMESPACE_KEY, self.namespace.as_ref()),
            (POD_KEY, self.pod.as_ref()),
            (NODE_KEY, self.node.as_ref()),
        ]
        .into_iter()
        .filter_map(|(key, name)| name.map(|name| (key, name)))
    }

    pub fn matches(&self, attributes: &BTreeMap<Box<str>, Box<str>>) -> bool {
        self.exact()
            .all(|(key, name)| attributes.get(key).map(|held| &**held) == Some(name.as_str()))
            && self
                .workload
                .as_ref()
                .is_none_or(|workload| workload.matches(attributes))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeenWorkload {
    pub kind: WorkloadKind,
    pub name: Box<str>,
    pub namespace: Box<str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeenPod {
    pub name: Box<str>,
    pub namespace: Box<str>,
    pub node: Box<str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Placements {
    pub clusters: Vec<Box<str>>,
    pub namespaces: Vec<Box<str>>,
    pub workloads: Vec<SeenWorkload>,
    pub pods: Vec<SeenPod>,
    pub nodes: Vec<Box<str>>,
}

impl Placements {
    #[must_use]
    pub fn capped(mut self) -> Self {
        self.clusters.truncate(MAX_PLACEMENTS);
        self.namespaces.truncate(MAX_PLACEMENTS);
        self.workloads.truncate(MAX_PLACEMENTS);
        self.pods.truncate(MAX_PLACEMENTS);
        self.nodes.truncate(MAX_PLACEMENTS);
        self
    }
}
