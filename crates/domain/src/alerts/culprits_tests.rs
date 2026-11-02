use std::collections::BTreeMap;

use super::culprits::{CulpritPod, Culprits, MAX_CULPRIT_FACETS, MAX_CULPRIT_LABELS};
use super::value_objects::Labels;

fn pod(name: &str, node: &str, count: u64) -> CulpritPod {
    CulpritPod {
        name: name.into(),
        node: Some(node.into()),
        count,
    }
}

fn names(culprits: &Culprits) -> Vec<&str> {
    culprits.pods().iter().map(|pod| &*pod.name).collect()
}

#[test]
fn seven_pods_are_cut_to_five_and_the_rest_counted() {
    let pods: Vec<CulpritPod> = (1..=7)
        .map(|index| pod(&format!("pod-{index}"), "node-a", index))
        .collect();
    let culprits = Culprits::new(pods, 7, Vec::new(), Vec::new(), &Labels::none());
    assert_eq!(culprits.pods().len(), 5);
    assert_eq!(culprits.others(), 2);
    assert_eq!(
        names(&culprits),
        ["pod-7", "pod-6", "pod-5", "pod-4", "pod-3"]
    );
}

#[test]
fn pods_are_ordered_by_count_then_name() {
    let culprits = Culprits::new(
        vec![
            pod("b", "n", 2),
            pod("c", "n", 9),
            pod("a", "n", 2),
            pod("d", "n", 1),
        ],
        4,
        Vec::new(),
        Vec::new(),
        &Labels::none(),
    );
    assert_eq!(names(&culprits), ["c", "a", "b", "d"]);
    assert_eq!(culprits.others(), 0);
}

#[test]
fn a_reader_returning_more_pods_than_the_cap_never_names_more_than_five() {
    let pods: Vec<CulpritPod> = (0..40).map(|i| pod(&format!("p{i:02}"), "n", 1)).collect();
    let culprits = Culprits::new(pods, 40, Vec::new(), Vec::new(), &Labels::none());
    assert_eq!(culprits.pods().len(), 5);
    assert_eq!(culprits.others(), 35);
}

#[test]
fn a_total_below_what_was_named_leaves_no_others() {
    let culprits = Culprits::new(
        vec![pod("a", "n", 1), pod("b", "n", 1)],
        1,
        Vec::new(),
        Vec::new(),
        &Labels::none(),
    );
    assert_eq!(culprits.others(), 0);
}

#[test]
fn nodes_and_versions_are_distinct_blank_free_and_bounded() {
    let many: Vec<Box<str>> = (0..9).map(|i| format!("v{i}").into()).collect();
    let mut nodes = vec!["a".into(), "".into(), "a".into(), "b".into()];
    nodes.extend(many.clone());
    let culprits = Culprits::new(Vec::new(), 0, nodes, many, &Labels::none());
    assert_eq!(&*culprits.nodes()[0], "a");
    assert_eq!(&*culprits.nodes()[1], "b");
    assert_eq!(culprits.nodes().len(), MAX_CULPRIT_FACETS);
    assert_eq!(culprits.versions().len(), MAX_CULPRIT_FACETS);
}

#[test]
fn labels_are_bounded() {
    let labels = Labels::new(
        (0..9)
            .map(|i| (format!("k{i}").into(), format!("v{i}").into()))
            .collect::<BTreeMap<Box<str>, Box<str>>>(),
    );
    let culprits = Culprits::of_labels(&labels);
    assert_eq!(culprits.labels().pairs().len(), MAX_CULPRIT_LABELS);
    assert!(!culprits.is_empty());
}

#[test]
fn nothing_known_is_empty() {
    assert!(Culprits::of_labels(&Labels::none()).is_empty());
    assert!(Culprits::default().is_empty());
}
