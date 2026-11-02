use std::collections::BTreeMap;

use super::errors::BadPlacement;
use super::value_objects::{
    CLUSTER_KEY, MAX_PLACEMENT_BYTES, MAX_PLACEMENTS, NAMESPACE_KEY, NODE_KEY, POD_KEY, Placement,
    PlacementName, Placements, SeenPod, SeenWorkload, Workload, WorkloadKind,
};

fn attributes(pairs: &[(&str, &str)]) -> BTreeMap<Box<str>, Box<str>> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).into(), (*value).into()))
        .collect()
}

fn named(text: &str) -> PlacementName {
    PlacementName::try_from(text).expect("a name")
}

#[test]
fn a_name_is_trimmed_and_kept_when_it_reads_like_a_kubernetes_name() {
    for good in [
        "checkout-7d9f8b6c5-x2k4q",
        "ip-10-0-1-2.eu-west-1.compute.internal",
        "prod_EU",
        "a",
    ] {
        assert_eq!(named(good).as_str(), good);
    }
    assert_eq!(named("  payments ").as_str(), "payments");
}

#[test]
fn a_blank_name_is_refused() {
    assert_eq!(PlacementName::try_from(""), Err(BadPlacement::Blank));
    assert_eq!(PlacementName::try_from("   "), Err(BadPlacement::Blank));
}

#[test]
fn a_name_longer_than_a_dns_name_is_refused() {
    let longest = "a".repeat(MAX_PLACEMENT_BYTES);
    let longer = "a".repeat(MAX_PLACEMENT_BYTES + 1);

    assert_eq!(named(&longest).as_str(), longest);
    assert_eq!(
        PlacementName::try_from(longer.as_str()),
        Err(BadPlacement::TooLong {
            max: MAX_PLACEMENT_BYTES
        })
    );
}

#[test]
fn a_name_with_characters_kubernetes_never_puts_in_one_is_refused() {
    for bad in [
        "pod name",
        "pod'; DROP TABLE spans",
        "-leading",
        "trailing-",
        ".dot",
        "é",
        "a/b",
        "a:b",
    ] {
        assert_eq!(
            PlacementName::try_from(bad),
            Err(BadPlacement::Characters),
            "{bad}"
        );
    }
}

#[test]
fn every_workload_kind_names_its_resource_attribute() {
    assert_eq!(WorkloadKind::Deployment.key(), "k8s.deployment.name");
    assert_eq!(WorkloadKind::StatefulSet.key(), "k8s.statefulset.name");
    assert_eq!(WorkloadKind::DaemonSet.key(), "k8s.daemonset.name");
    assert_eq!(WorkloadKind::Job.key(), "k8s.job.name");
    assert_eq!(WorkloadKind::CronJob.key(), "k8s.cronjob.name");
    for kind in WorkloadKind::ALL {
        assert_eq!(WorkloadKind::parse(kind.name()), Some(*kind));
    }
    assert_eq!(WorkloadKind::parse("replicaset"), None);
}

#[test]
fn a_workload_names_a_kind_before_a_colon_or_none_at_all() {
    let kinded = Workload::try_from("statefulset:postgres").expect("a kinded workload");
    let bare = Workload::try_from("postgres").expect("a bare workload");
    let shouted = Workload::try_from("Deployment:api").expect("a kind in any case");

    assert_eq!(kinded.kind(), Some(WorkloadKind::StatefulSet));
    assert_eq!(kinded.name().as_str(), "postgres");
    assert_eq!(kinded.to_string(), "statefulset:postgres");
    assert_eq!(bare.kind(), None);
    assert_eq!(bare.name().as_str(), "postgres");
    assert_eq!(bare.to_string(), "postgres");
    assert_eq!(shouted.kind(), Some(WorkloadKind::Deployment));
}

#[test]
fn a_workload_of_an_unknown_kind_or_without_a_name_is_refused() {
    assert_eq!(
        Workload::try_from("replicaset:api"),
        Err(BadPlacement::Kind)
    );
    assert_eq!(Workload::try_from("deployment:"), Err(BadPlacement::Blank));
    assert_eq!(
        Workload::try_from("deployment:a b"),
        Err(BadPlacement::Characters)
    );
    assert_eq!(Workload::try_from(""), Err(BadPlacement::Blank));
}

#[test]
fn an_empty_placement_matches_everything() {
    let placement = Placement::default();

    assert!(placement.is_empty());
    assert!(placement.matches(&attributes(&[])));
}

#[test]
fn a_placement_matches_only_the_attributes_that_carry_every_part_of_it() {
    let placement = Placement {
        cluster: Some(named("prod")),
        namespace: Some(named("shop")),
        workload: Some(Workload::try_from("checkout").expect("a workload")),
        pod: Some(named("checkout-1")),
        node: Some(named("node-a")),
    };
    let there = attributes(&[
        (CLUSTER_KEY, "prod"),
        (NAMESPACE_KEY, "shop"),
        ("k8s.deployment.name", "checkout"),
        (POD_KEY, "checkout-1"),
        (NODE_KEY, "node-a"),
    ]);
    let other_node = attributes(&[
        (CLUSTER_KEY, "prod"),
        (NAMESPACE_KEY, "shop"),
        ("k8s.deployment.name", "checkout"),
        (POD_KEY, "checkout-1"),
        (NODE_KEY, "node-b"),
    ]);

    assert!(!placement.is_empty());
    assert!(placement.matches(&there));
    assert!(!placement.matches(&other_node));
    assert!(!placement.matches(&attributes(&[])));
}

#[test]
fn a_bare_workload_matches_the_workload_that_owns_the_pod_and_a_kinded_one_its_own_key() {
    let cron_pod = attributes(&[
        ("k8s.cronjob.name", "backup"),
        ("k8s.job.name", "backup-28472"),
    ]);
    let bare = |name: &str| Placement {
        workload: Some(Workload::try_from(name).expect("a workload")),
        ..Placement::default()
    };

    assert_eq!(
        Workload::owner_of(&cron_pod),
        Some((WorkloadKind::CronJob, "backup"))
    );
    assert!(bare("backup").matches(&cron_pod));
    assert!(!bare("backup-28472").matches(&cron_pod));
    assert!(bare("job:backup-28472").matches(&cron_pod));
    assert!(bare("cronjob:backup").matches(&cron_pod));
    assert!(!bare("deployment:backup").matches(&cron_pod));
    assert_eq!(Workload::owner_of(&attributes(&[])), None);
}

#[test]
fn placements_seen_are_capped_at_five_hundred_of_each() {
    let many = |prefix: &str| -> Vec<Box<str>> {
        (0..MAX_PLACEMENTS + 3)
            .map(|n| format!("{prefix}-{n}").into())
            .collect()
    };
    let seen = Placements {
        clusters: many("c"),
        namespaces: many("n"),
        workloads: (0..=MAX_PLACEMENTS)
            .map(|n| SeenWorkload {
                kind: WorkloadKind::Deployment,
                name: format!("w-{n}").into(),
                namespace: "shop".into(),
            })
            .collect(),
        pods: (0..=MAX_PLACEMENTS)
            .map(|n| SeenPod {
                name: format!("p-{n}").into(),
                namespace: "shop".into(),
                node: "node-a".into(),
            })
            .collect(),
        nodes: many("node"),
    }
    .capped();

    assert_eq!(seen.clusters.len(), MAX_PLACEMENTS);
    assert_eq!(seen.namespaces.len(), MAX_PLACEMENTS);
    assert_eq!(seen.workloads.len(), MAX_PLACEMENTS);
    assert_eq!(seen.pods.len(), MAX_PLACEMENTS);
    assert_eq!(seen.nodes.len(), MAX_PLACEMENTS);
}
