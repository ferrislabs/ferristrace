use super::entities::Observation;
use super::value_objects::{AlertSeverity, Comparison, Condition, Labels, NoDataPolicy};
use super::value_objects::{Definition, DefinitionParts, LlmGroup, LlmMeasure, LlmSignal, Signal};
use crate::catalogue::ProjectId;
use crate::explore::Window;
use crate::llm::{BucketTotals, GroupBy, GroupLatency, PriceBook, Spend, SpendRead};
use uuid::Uuid;

fn signal(measure: LlmMeasure, group_by: LlmGroup) -> LlmSignal {
    LlmSignal {
        measure,
        group_by,
        feature: None,
        customer: None,
        model: None,
        project: None,
    }
}

fn window() -> Window {
    Window::new(0, 3_600_000_000_000).expect("a window")
}

fn spend(keys: &[(&str, u64)]) -> Spend {
    let query = signal(LlmMeasure::Cost, LlmGroup::Feature).query(None, window());
    let read = SpendRead {
        buckets: keys
            .iter()
            .map(|(key, calls)| BucketTotals {
                key: (*key).into(),
                provider: "openai".into(),
                request_model: "gpt".into(),
                response_model: "".into(),
                slot: 0,
                calls: *calls,
                errors: 0,
                input_tokens: calls * 1_000,
                output_tokens: calls * 500,
                cached_input_tokens: 0,
            })
            .collect(),
        latencies: keys
            .iter()
            .map(|(key, _)| GroupLatency {
                key: (*key).into(),
                p50_nanos: 1_000_000,
                p95_nanos: 250_000_000,
            })
            .collect(),
        markers: Vec::new(),
    };
    Spend::priced(read, &PriceBook::new(&[]), &query)
}

fn only(found: &[Observation]) -> &Observation {
    assert_eq!(found.len(), 1, "{found:?}");
    &found[0]
}

#[test]
fn the_measure_picks_cost_tokens_calls_or_p95_in_milliseconds() {
    let held = spend(&[("a", 4)]);
    let value = |measure| {
        signal(measure, LlmGroup::None)
            .observe(&held)
            .first()
            .and_then(|seen| seen.value)
    };
    assert_eq!(value(LlmMeasure::Cost), Some(0.0));
    assert_eq!(value(LlmMeasure::Tokens), Some(6_000.0));
    assert_eq!(value(LlmMeasure::Calls), Some(4.0));
    assert_eq!(value(LlmMeasure::P95), Some(250.0));
}

#[test]
fn no_group_gives_one_unlabelled_observation_and_a_group_labels_each() {
    let held = spend(&[("a", 1), ("b", 2)]);

    let grouped = signal(LlmMeasure::Calls, LlmGroup::Feature).observe(&held);

    assert_eq!(grouped.len(), 2);
    let labels: Vec<Labels> = grouped.iter().map(|seen| seen.labels.clone()).collect();
    for key in ["a", "b"] {
        assert!(labels.iter().any(|held| {
            held.pairs().get("feature").map(AsRef::as_ref) == Some(key) && held.pairs().len() == 1
        }));
    }
    let customer = signal(LlmMeasure::Calls, LlmGroup::Customer).observe(&held);
    assert!(
        customer
            .iter()
            .all(|seen| seen.labels.pairs().contains_key("customer"))
    );
    let total = signal(LlmMeasure::Calls, LlmGroup::None).observe(&spend(&[("all", 3)]));
    assert!(only(&total).labels.is_empty());
}

#[test]
fn without_calls_there_is_nothing_observed() {
    assert!(
        signal(LlmMeasure::Cost, LlmGroup::None)
            .observe(&Spend::default())
            .is_empty()
    );
}

#[test]
fn the_query_carries_the_filters_the_group_and_the_narrower_project() {
    let mine = ProjectId::new(Uuid::from_u128(1));
    let other = ProjectId::new(Uuid::from_u128(2));
    let mut held = signal(LlmMeasure::Cost, LlmGroup::Model);
    held.feature = Some("ticket-summary".into());
    held.customer = Some("acme".into());
    held.model = Some("gpt-4o".into());
    held.project = Some(other);

    let query = held.query(Some(mine), window());

    assert_eq!(query.group_by, GroupBy::Model);
    assert_eq!(query.filter.feature.as_deref(), Some("ticket-summary"));
    assert_eq!(query.filter.customer.as_deref(), Some("acme"));
    assert_eq!(query.filter.model.as_deref(), Some("gpt-4o"));
    assert_eq!(query.filter.project, Some(mine));
    assert_eq!(held.query(None, window()).filter.project, Some(other));
    assert_eq!(
        signal(LlmMeasure::Cost, LlmGroup::None)
            .query(None, window())
            .group_by,
        GroupBy::Total
    );
}

fn definition(signal: LlmSignal) -> Result<Definition, super::errors::BadRule> {
    Definition::new(DefinitionParts {
        signal: Signal::Llm(signal),
        condition: Condition::new(Comparison::Above, 10.0).expect("a threshold"),
        window_secs: 3_600,
        pending_secs: 0,
        severity: AlertSeverity::Warning,
        no_data: NoDataPolicy::Ok,
        channels: Vec::new(),
        page: None,
        mentions: crate::Mentions::default(),
    })
}

#[test]
fn a_blank_filter_value_is_refused_and_a_plain_signal_is_accepted() {
    assert!(definition(signal(LlmMeasure::Cost, LlmGroup::None)).is_ok());
    let mut blank = signal(LlmMeasure::Cost, LlmGroup::None);
    blank.feature = Some("  ".into());
    assert_eq!(definition(blank), Err(super::errors::BadRule::LlmFilter));
}

#[test]
fn spellings_round_trip_and_unknown_ones_are_refused() {
    for measure in LlmMeasure::ALL {
        assert_eq!(LlmMeasure::parse(measure.as_str()), Some(*measure));
    }
    for group in LlmGroup::ALL {
        assert_eq!(LlmGroup::parse(group.as_str()), Some(*group));
    }
    assert_eq!(LlmMeasure::parse("spend"), None);
    assert_eq!(LlmGroup::parse("user"), None);
    assert_eq!(
        Signal::Llm(signal(LlmMeasure::Cost, LlmGroup::None)).kind(),
        "llm"
    );
    assert!(!Signal::Llm(signal(LlmMeasure::Cost, LlmGroup::None)).groups());
    assert!(Signal::Llm(signal(LlmMeasure::Cost, LlmGroup::Model)).groups());
}
