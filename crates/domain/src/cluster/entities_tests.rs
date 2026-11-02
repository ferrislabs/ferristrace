use super::entities::{ClusterSnapshot, Node, Pod, WorkloadSummary, assess};
use super::value_objects::{ClusterFilter, PodPhase, PodReason};
use crate::placement::{PlacementName, WorkloadKind};

fn a_pod(phase: Option<PodPhase>) -> Pod {
    Pod {
        phase,
        ..Pod::named("checkout-1", "shop")
    }
}

#[test]
fn a_running_ready_pod_on_a_ready_node_is_healthy() {
    let pod = Pod {
        ready: Some(true),
        restarts_total: 3,
        ..a_pod(Some(PodPhase::Running))
    };

    assert_eq!(assess(&pod, Some(true)), (false, None));
    assert_eq!(assess(&pod, None), (false, None));
}

#[test]
fn every_case_of_the_assessment_table_names_its_reason() {
    let restarting = Pod {
        restarts_in_window: 2,
        ready: Some(false),
        ..a_pod(Some(PodPhase::Running))
    };
    let not_ready = Pod {
        ready: Some(false),
        ..a_pod(Some(PodPhase::Running))
    };
    let table = [
        (a_pod(Some(PodPhase::Running)), Some(true), None),
        (a_pod(Some(PodPhase::Succeeded)), Some(true), None),
        (a_pod(None), None, None),
        (
            a_pod(Some(PodPhase::Pending)),
            Some(true),
            Some(PodReason::Phase(PodPhase::Pending)),
        ),
        (
            a_pod(Some(PodPhase::Failed)),
            None,
            Some(PodReason::Phase(PodPhase::Failed)),
        ),
        (
            a_pod(Some(PodPhase::Unknown)),
            None,
            Some(PodReason::Phase(PodPhase::Unknown)),
        ),
        (restarting, Some(true), Some(PodReason::Restarting)),
        (not_ready, Some(true), Some(PodReason::NotReady)),
        (
            a_pod(Some(PodPhase::Running)),
            Some(false),
            Some(PodReason::NodeNotReady),
        ),
    ];

    for (pod, node_ready, reason) in table {
        assert_eq!(
            assess(&pod, node_ready),
            (reason.is_some(), reason),
            "{pod:?} on a node ready {node_ready:?}"
        );
    }
}

#[test]
fn a_pending_pod_names_its_phase_rather_than_its_readiness() {
    let pending = Pod {
        ready: Some(false),
        ..a_pod(Some(PodPhase::Pending))
    };

    assert_eq!(
        assess(&pending, Some(true)),
        (true, Some(PodReason::Phase(PodPhase::Pending)))
    );
}

#[test]
fn reasons_and_phases_read_as_the_cluster_view_shows_them() {
    assert_eq!(PodReason::Restarting.to_string(), "restarting");
    assert_eq!(PodReason::NotReady.to_string(), "not ready");
    assert_eq!(PodReason::NodeNotReady.to_string(), "node not ready");
    assert_eq!(PodReason::Phase(PodPhase::Failed).to_string(), "failed");
    assert_eq!(PodPhase::Running.name(), "Running");
}

#[test]
fn a_phase_is_read_from_the_value_the_k8s_cluster_receiver_reports() {
    assert_eq!(PodPhase::of_value(1.0), Some(PodPhase::Pending));
    assert_eq!(PodPhase::of_value(2.0), Some(PodPhase::Running));
    assert_eq!(PodPhase::of_value(3.0), Some(PodPhase::Succeeded));
    assert_eq!(PodPhase::of_value(4.0), Some(PodPhase::Failed));
    assert_eq!(PodPhase::of_value(5.0), Some(PodPhase::Unknown));
    assert_eq!(PodPhase::of_value(0.0), None);
    assert_eq!(PodPhase::of_value(f64::NAN), None);
}

fn on(name: &str, node: &str, workload: &str) -> Pod {
    Pod {
        node: Some(node.into()),
        workload: Some((WorkloadKind::Deployment, workload.into())),
        phase: Some(PodPhase::Running),
        ..Pod::named(name, "shop")
    }
}

#[test]
fn a_settled_snapshot_counts_pods_per_node_and_workload_and_judges_each_pod() {
    let snapshot = ClusterSnapshot {
        clusters: vec!["prod".into(), "".into(), "prod".into()],
        namespaces: vec!["shop".into()],
        nodes: vec![
            Node {
                ready: Some(false),
                pods: 9,
                ..Node::named("node-b")
            },
            Node {
                ready: Some(true),
                ..Node::named("node-a")
            },
        ],
        workloads: vec![WorkloadSummary {
            cluster: "".into(),
            kind: WorkloadKind::Deployment,
            name: "checkout".into(),
            namespace: "shop".into(),
            desired: Some(3),
            ready: Some(2),
            pods: 0,
        }],
        pods: vec![
            on("checkout-2", "node-b", "checkout"),
            on("checkout-1", "node-a", "checkout"),
            Pod {
                services: vec!["mailer".into(), "api".into(), "mailer".into()],
                ..on("mailer-1", "node-c", "mailer")
            },
        ],
        seen_ms: Some(5),
    }
    .settled(&ClusterFilter::default());

    assert_eq!(snapshot.clusters, vec![Box::from("prod")]);
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .map(|node| (&*node.name, node.pods, node.ready))
            .collect::<Vec<_>>(),
        vec![
            ("node-a", 1, Some(true)),
            ("node-b", 1, Some(false)),
            ("node-c", 1, None)
        ]
    );
    assert_eq!(
        snapshot
            .workloads
            .iter()
            .map(|workload| (&*workload.name, workload.desired, workload.pods))
            .collect::<Vec<_>>(),
        vec![("checkout", Some(3), 2), ("mailer", None, 1)]
    );
    assert_eq!(
        snapshot
            .pods
            .iter()
            .map(|pod| (&*pod.name, pod.unhealthy, pod.reason))
            .collect::<Vec<_>>(),
        vec![
            ("checkout-1", false, None),
            ("checkout-2", true, Some(PodReason::NodeNotReady)),
            ("mailer-1", false, None)
        ]
    );
    assert_eq!(
        snapshot.pods[2].services,
        vec![Box::from("api"), Box::from("mailer")]
    );
}

#[test]
fn a_namespace_keeps_only_the_nodes_its_pods_run_on() {
    let snapshot = ClusterSnapshot {
        nodes: vec![Node::named("node-a"), Node::named("node-b")],
        pods: vec![on("checkout-1", "node-a", "checkout")],
        ..ClusterSnapshot::default()
    }
    .settled(&ClusterFilter {
        namespace: Some(PlacementName::try_from("shop").expect("a namespace")),
        ..ClusterFilter::default()
    });

    assert_eq!(
        snapshot
            .nodes
            .iter()
            .map(|node| &*node.name)
            .collect::<Vec<_>>(),
        vec!["node-a"]
    );
}

#[test]
fn a_workload_of_the_same_name_in_two_clusters_counts_its_pods_apart() {
    let snapshot = ClusterSnapshot {
        pods: vec![
            Pod {
                cluster: "prod".into(),
                ..on("checkout-1", "node-a", "checkout")
            },
            Pod {
                cluster: "prod".into(),
                ..on("checkout-2", "node-a", "checkout")
            },
            Pod {
                cluster: "staging".into(),
                ..on("checkout-3", "node-s", "checkout")
            },
        ],
        ..ClusterSnapshot::default()
    }
    .settled(&ClusterFilter::default());

    assert_eq!(
        snapshot
            .workloads
            .iter()
            .map(|workload| (&*workload.cluster, &*workload.name, workload.pods))
            .collect::<Vec<_>>(),
        vec![("prod", "checkout", 2), ("staging", "checkout", 1)]
    );
}
