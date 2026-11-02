use super::value_objects::Labels;

pub const MAX_CULPRIT_PODS: usize = 5;
pub const MAX_CULPRIT_FACETS: usize = 5;
pub const MAX_CULPRIT_LABELS: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CulpritPod {
    pub name: Box<str>,
    pub node: Option<Box<str>>,
    pub count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Culprits {
    pods: Vec<CulpritPod>,
    others: u64,
    nodes: Vec<Box<str>>,
    versions: Vec<Box<str>>,
    labels: Labels,
}

impl Culprits {
    pub fn new(
        mut pods: Vec<CulpritPod>,
        distinct_pods: u64,
        nodes: Vec<Box<str>>,
        versions: Vec<Box<str>>,
        labels: &Labels,
    ) -> Self {
        pods.retain(|pod| !pod.name.is_empty());
        pods.sort_by(|one, other| {
            other
                .count
                .cmp(&one.count)
                .then_with(|| one.name.cmp(&other.name))
        });
        pods.dedup_by(|later, earlier| later.name == earlier.name);
        pods.truncate(MAX_CULPRIT_PODS);
        let named = u64::try_from(pods.len()).unwrap_or(u64::MAX);
        Self {
            others: distinct_pods.saturating_sub(named),
            pods,
            nodes: distinct(nodes),
            versions: distinct(versions),
            labels: Labels::new(
                labels
                    .pairs()
                    .iter()
                    .take(MAX_CULPRIT_LABELS)
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
            ),
        }
    }

    pub fn of_labels(labels: &Labels) -> Self {
        Self::new(Vec::new(), 0, Vec::new(), Vec::new(), labels)
    }

    pub fn pods(&self) -> &[CulpritPod] {
        &self.pods
    }

    pub fn others(&self) -> u64 {
        self.others
    }

    pub fn nodes(&self) -> &[Box<str>] {
        &self.nodes
    }

    pub fn versions(&self) -> &[Box<str>] {
        &self.versions
    }

    pub fn labels(&self) -> &Labels {
        &self.labels
    }

    pub fn is_empty(&self) -> bool {
        self.pods.is_empty()
            && self.others == 0
            && self.nodes.is_empty()
            && self.versions.is_empty()
            && self.labels.is_empty()
    }
}

fn distinct(names: Vec<Box<str>>) -> Vec<Box<str>> {
    let mut kept: Vec<Box<str>> = Vec::with_capacity(MAX_CULPRIT_FACETS);
    for name in names {
        if kept.len() == MAX_CULPRIT_FACETS {
            break;
        }
        if !name.is_empty() && !kept.contains(&name) {
            kept.push(name);
        }
    }
    kept
}
