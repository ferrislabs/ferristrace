use std::collections::BTreeMap;

use super::entities::{PromptMarker, Spend, SpendGroup, SpendRead};
use super::pricing::PriceBook;
use super::value_objects::{Cost, SpendQuery};

const NANOS_PER_MILLI: u64 = 1_000_000;

impl Spend {
    pub fn priced(read: SpendRead, book: &PriceBook<'_>, query: &SpendQuery) -> Self {
        let step = query.step_nanos();
        let starts: Vec<u64> = query.slot_starts().collect();
        let mut groups: BTreeMap<Box<str>, SpendGroup> = BTreeMap::new();
        for bucket in read.buckets {
            let group = groups
                .entry(bucket.key.clone())
                .or_insert_with(|| SpendGroup {
                    key: bucket.key.clone(),
                    cost: Cost::default(),
                    unpriced: 0,
                    calls: 0,
                    input_tokens: 0,
                    output_tokens: 0,
                    p50_nanos: 0,
                    p95_nanos: 0,
                    errors: 0,
                    points: starts
                        .iter()
                        .map(|start| (*start, Cost::default()))
                        .collect(),
                });
            group.calls += bucket.calls;
            group.errors += bucket.errors;
            group.input_tokens += bucket.input_tokens;
            group.output_tokens += bucket.output_tokens;
            let slot = usize::try_from(bucket.slot)
                .unwrap_or(usize::MAX)
                .min(starts.len() - 1);
            let start = starts[slot];
            let found = book.of_models(
                &bucket.provider,
                &bucket.response_model,
                &bucket.request_model,
                start / NANOS_PER_MILLI,
            );
            match found {
                None => group.unpriced += bucket.calls,
                Some(price) => {
                    let cost = price.rates.cost(
                        bucket.input_tokens,
                        bucket.output_tokens,
                        bucket.cached_input_tokens,
                    );
                    group.cost += cost;
                    group.points[slot].1 += cost;
                }
            }
        }
        for latency in read.latencies {
            if let Some(group) = groups.get_mut(&latency.key) {
                group.p50_nanos = latency.p50_nanos;
                group.p95_nanos = latency.p95_nanos;
            }
        }
        let mut listed: Vec<SpendGroup> = groups.into_values().collect();
        listed.sort_by(|left, right| {
            right
                .cost
                .cmp(&left.cost)
                .then_with(|| right.calls.cmp(&left.calls))
                .then_with(|| left.key.cmp(&right.key))
        });
        let mut markers: Vec<PromptMarker> = read.markers;
        markers.sort_by(|left, right| {
            left.time_unix_nanos
                .cmp(&right.time_unix_nanos)
                .then_with(|| left.feature.cmp(&right.feature))
                .then_with(|| left.prompt_version.cmp(&right.prompt_version))
        });
        Self {
            step_nanos: step,
            groups: listed,
            markers,
        }
    }
}
