use std::collections::BTreeMap;

use ferristrace_domain::Culprits;
use serde::Serialize;

#[derive(Debug, Serialize)]
struct PodBody {
    name: Box<str>,
    node: Option<Box<str>>,
    count: u64,
}

#[derive(Debug, Serialize)]
pub struct ContextBody {
    pods: Vec<PodBody>,
    others: u64,
    nodes: Vec<Box<str>>,
    versions: Vec<Box<str>>,
    labels: BTreeMap<Box<str>, Box<str>>,
}

impl From<&Culprits> for ContextBody {
    fn from(culprits: &Culprits) -> Self {
        Self {
            pods: culprits
                .pods()
                .iter()
                .map(|pod| PodBody {
                    name: pod.name.clone(),
                    node: pod.node.clone(),
                    count: pod.count,
                })
                .collect(),
            others: culprits.others(),
            nodes: culprits.nodes().to_vec(),
            versions: culprits.versions().to_vec(),
            labels: culprits.labels().pairs().clone(),
        }
    }
}
