use std::collections::{BTreeMap, BTreeSet};

use super::value_objects::{ClusterFilter, PodPhase, PodReason};
use crate::placement::WorkloadKind;

#[derive(Debug, Clone, PartialEq)]
pub struct Pod {
    pub name: Box<str>,
    pub namespace: Box<str>,
    pub cluster: Box<str>,
    pub workload: Option<(WorkloadKind, Box<str>)>,
    pub node: Option<Box<str>>,
    pub phase: Option<PodPhase>,
    pub restarts_in_window: u64,
    pub restarts_total: u64,
    pub cpu_cores: Option<f64>,
    pub memory_bytes: Option<u64>,
    pub ready: Option<bool>,
    pub services: Vec<Box<str>>,
    pub unhealthy: bool,
    pub reason: Option<PodReason>,
}

impl Pod {
    pub fn named(name: &str, namespace: &str) -> Self {
        Self {
            name: name.into(),
            namespace: namespace.into(),
            cluster: Box::from(""),
            workload: None,
            node: None,
            phase: None,
            restarts_in_window: 0,
            restarts_total: 0,
            cpu_cores: None,
            memory_bytes: None,
            ready: None,
            services: Vec::new(),
            unhealthy: false,
            reason: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub name: Box<str>,
    pub ready: Option<bool>,
    pub cpu_cores: Option<f64>,
    pub memory_bytes: Option<u64>,
    pub memory_available: Option<u64>,
    pub pods: u64,
}

impl Node {
    pub fn named(name: &str) -> Self {
        Self {
            name: name.into(),
            ready: None,
            cpu_cores: None,
            memory_bytes: None,
            memory_available: None,
            pods: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadSummary {
    pub cluster: Box<str>,
    pub kind: WorkloadKind,
    pub name: Box<str>,
    pub namespace: Box<str>,
    pub desired: Option<u64>,
    pub ready: Option<u64>,
    pub pods: u64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClusterSnapshot {
    pub clusters: Vec<Box<str>>,
    pub namespaces: Vec<Box<str>>,
    pub nodes: Vec<Node>,
    pub workloads: Vec<WorkloadSummary>,
    pub pods: Vec<Pod>,
    pub seen_ms: Option<u64>,
}

pub fn assess(pod: &Pod, node_ready: Option<bool>) -> (bool, Option<PodReason>) {
    let reason = if pod.restarts_in_window > 0 {
        Some(PodReason::Restarting)
    } else if pod.phase == Some(PodPhase::Running) && pod.ready == Some(false) {
        Some(PodReason::NotReady)
    } else if let Some(phase) = pod.phase.filter(|phase| !phase.is_healthy()) {
        Some(PodReason::Phase(phase))
    } else if node_ready == Some(false) {
        Some(PodReason::NodeNotReady)
    } else {
        None
    };
    (reason.is_some(), reason)
}

type WorkloadKey = (Box<str>, Box<str>, WorkloadKind, Box<str>);

impl ClusterSnapshot {
    #[must_use]
    pub fn settled(mut self, filter: &ClusterFilter) -> Self {
        let mut nodes: BTreeMap<Box<str>, Node> = self
            .nodes
            .drain(..)
            .map(|node| (node.name.clone(), Node { pods: 0, ..node }))
            .collect();
        let mut workloads: BTreeMap<WorkloadKey, WorkloadSummary> = self
            .workloads
            .drain(..)
            .map(|workload| {
                (
                    (
                        workload.cluster.clone(),
                        workload.namespace.clone(),
                        workload.kind,
                        workload.name.clone(),
                    ),
                    WorkloadSummary {
                        pods: 0,
                        ..workload
                    },
                )
            })
            .collect();
        for pod in &mut self.pods {
            let node_ready = pod.node.as_ref().and_then(|name| {
                let node = nodes
                    .entry(name.clone())
                    .or_insert_with(|| Node::named(name));
                node.pods += 1;
                node.ready
            });
            if let Some((kind, name)) = &pod.workload {
                workloads
                    .entry((
                        pod.cluster.clone(),
                        pod.namespace.clone(),
                        *kind,
                        name.clone(),
                    ))
                    .or_insert_with(|| WorkloadSummary {
                        cluster: pod.cluster.clone(),
                        kind: *kind,
                        name: name.clone(),
                        namespace: pod.namespace.clone(),
                        desired: None,
                        ready: None,
                        pods: 0,
                    })
                    .pods += 1;
            }
            (pod.unhealthy, pod.reason) = assess(pod, node_ready);
            pod.services.sort_unstable();
            pod.services.dedup();
        }
        if filter.namespace.is_some() {
            nodes.retain(|_, node| node.pods > 0);
        }
        self.pods.sort_by(|left, right| {
            (&left.namespace, &left.name, &left.cluster).cmp(&(
                &right.namespace,
                &right.name,
                &right.cluster,
            ))
        });
        self.nodes = nodes.into_values().collect();
        self.workloads = workloads.into_values().collect();
        self.clusters = sorted(self.clusters);
        self.namespaces = sorted(self.namespaces);
        self
    }
}

fn sorted(names: Vec<Box<str>>) -> Vec<Box<str>> {
    names
        .into_iter()
        .filter(|name| !name.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
