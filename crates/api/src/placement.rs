use ferristrace_domain::{Placement, PlacementName, Placements, Workload};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the {0} parameter is not a valid placement")]
pub struct Misplaced(pub &'static str);

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct PlacementBody {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) cluster: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) workload: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) pod: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) node: Option<String>,
}

fn given(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|held| !held.is_empty())
}

fn named(text: Option<&str>, field: &'static str) -> Result<Option<PlacementName>, Misplaced> {
    given(text)
        .map(|held| PlacementName::try_from(held).map_err(|_| Misplaced(field)))
        .transpose()
}

impl PlacementBody {
    pub(crate) fn of(placement: &Placement) -> Self {
        Self {
            cluster: placement.cluster.as_ref().map(ToString::to_string),
            namespace: placement.namespace.as_ref().map(ToString::to_string),
            workload: placement.workload.as_ref().map(ToString::to_string),
            pod: placement.pod.as_ref().map(ToString::to_string),
            node: placement.node.as_ref().map(ToString::to_string),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.cluster.is_none()
            && self.namespace.is_none()
            && self.workload.is_none()
            && self.pod.is_none()
            && self.node.is_none()
    }

    pub(crate) fn hold(&mut self, key: &str, value: &str) {
        let slot = match key {
            "cluster" => &mut self.cluster,
            "namespace" => &mut self.namespace,
            "workload" => &mut self.workload,
            "pod" => &mut self.pod,
            "node" => &mut self.node,
            _ => return,
        };
        *slot = Some(value.to_owned());
    }

    pub(crate) fn placement(&self) -> Result<Placement, Misplaced> {
        Ok(Placement {
            cluster: named(self.cluster.as_deref(), "cluster")?,
            namespace: named(self.namespace.as_deref(), "namespace")?,
            workload: given(self.workload.as_deref())
                .map(|held| Workload::try_from(held).map_err(|_| Misplaced("workload")))
                .transpose()?,
            pod: named(self.pod.as_deref(), "pod")?,
            node: named(self.node.as_deref(), "node")?,
        })
    }
}

#[derive(Debug, Serialize)]
pub struct Placed {
    clusters: Vec<String>,
    namespaces: Vec<String>,
    workloads: Vec<PlacedWorkload>,
    pods: Vec<PlacedPod>,
    nodes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct PlacedWorkload {
    kind: &'static str,
    name: String,
    namespace: String,
}

#[derive(Debug, Serialize)]
struct PlacedPod {
    name: String,
    namespace: String,
    node: String,
}

fn strings(held: Vec<Box<str>>) -> Vec<String> {
    held.into_iter().map(str::into_string).collect()
}

impl From<Placements> for Placed {
    fn from(seen: Placements) -> Self {
        Self {
            clusters: strings(seen.clusters),
            namespaces: strings(seen.namespaces),
            workloads: seen
                .workloads
                .into_iter()
                .map(|workload| PlacedWorkload {
                    kind: workload.kind.name(),
                    name: workload.name.into_string(),
                    namespace: workload.namespace.into_string(),
                })
                .collect(),
            pods: seen
                .pods
                .into_iter()
                .map(|pod| PlacedPod {
                    name: pod.name.into_string(),
                    namespace: pod.namespace.into_string(),
                    node: pod.node.into_string(),
                })
                .collect(),
            nodes: strings(seen.nodes),
        }
    }
}
