use super::entities::{BucketTotals, GroupLatency, Price, PromptMarker, Spend, SpendRead};
use super::pricing::PriceBook;
use super::value_objects::{GroupBy, ModelPattern, PriceId, Provider, Rates, SpendQuery};
use crate::explore::Window;

const MINUTE_NANOS: u64 = 60_000_000_000;

fn price(model: &str, input: u64, effective_from_ms: u64) -> Price {
    Price {
        id: PriceId::fresh(),
        provider: Provider::try_from("openai").expect("a provider"),
        model: ModelPattern::try_from(model).expect("a model"),
        rates: Rates::new(input, 0, None).expect("rates"),
        effective_from_ms,
    }
}

fn query() -> SpendQuery {
    SpendQuery::over(Window::new(0, 60 * MINUTE_NANOS).expect("a window"))
}

fn bucket(key: &str, model: &str, slot: u64, calls: u64) -> BucketTotals {
    BucketTotals {
        key: key.into(),
        provider: "OpenAI".into(),
        request_model: model.into(),
        response_model: String::new().into(),
        slot,
        calls,
        errors: 0,
        input_tokens: calls * 1_000_000,
        output_tokens: 0,
        cached_input_tokens: 0,
    }
}

fn read(buckets: Vec<BucketTotals>) -> SpendRead {
    SpendRead {
        buckets,
        ..SpendRead::default()
    }
}

#[test]
fn a_bucket_is_priced_at_its_start_so_a_price_change_splits_the_series() {
    let prices = [
        price("gpt-4o", 2_000_000, 0),
        price("gpt-4o", 4_000_000, 1_800_000),
    ];
    let asked = query();
    let step = asked.step_nanos();
    assert_eq!(step, MINUTE_NANOS);

    let spend = Spend::priced(
        read(vec![
            bucket("a", "gpt-4o", 29, 1),
            bucket("a", "gpt-4o", 30, 1),
            bucket("a", "gpt-4o", 31, 1),
        ]),
        &PriceBook::new(&prices),
        &asked,
    );

    let group = &spend.groups[0];
    assert_eq!(group.points.len(), 60);
    assert!((group.points[29].1.dollars() - 2.0).abs() < 1e-9);
    assert!((group.points[30].1.dollars() - 4.0).abs() < 1e-9);
    assert!((group.points[31].1.dollars() - 4.0).abs() < 1e-9);
    assert!((group.cost.dollars() - 10.0).abs() < 1e-9);
    assert_eq!(group.points[30].0, 30 * step);
    assert_eq!(spend.step_nanos, step);
}

#[test]
fn calls_without_a_price_are_counted_unpriced_and_cost_nothing() {
    let prices = [price("gpt-4o", 2_000_000, 0)];

    let spend = Spend::priced(
        read(vec![
            bucket("a", "gpt-4o", 1, 2),
            bucket("a", "mystery", 1, 3),
        ]),
        &PriceBook::new(&prices),
        &query(),
    );

    let group = &spend.groups[0];
    assert_eq!(group.calls, 5);
    assert_eq!(group.unpriced, 3);
    assert!((group.cost.dollars() - 4.0).abs() < 1e-9);
    assert_eq!(group.input_tokens, 5_000_000);
}

#[test]
fn a_price_starting_after_the_bucket_leaves_it_unpriced() {
    let prices = [price("gpt-4o", 2_000_000, 10 * 60_000)];

    let spend = Spend::priced(
        read(vec![
            bucket("a", "gpt-4o", 9, 1),
            bucket("a", "gpt-4o", 10, 1),
        ]),
        &PriceBook::new(&prices),
        &query(),
    );

    assert_eq!(spend.groups[0].unpriced, 1);
    assert!((spend.groups[0].cost.dollars() - 2.0).abs() < 1e-9);
}

#[test]
fn the_response_model_is_priced_before_the_request_model() {
    let prices = [
        price("gpt-4o", 1_000_000, 0),
        price("gpt-4o-2024*", 3_000_000, 0),
    ];
    let mut row = bucket("a", "gpt-4o", 0, 1);
    row.response_model = "gpt-4o-2024-08-06".into();

    let spend = Spend::priced(read(vec![row]), &PriceBook::new(&prices), &query());

    assert!((spend.groups[0].cost.dollars() - 3.0).abs() < 1e-9);
}

#[test]
fn groups_are_ordered_by_cost_and_carry_their_latency_and_errors() {
    let prices = [price("gpt-4o", 1_000_000, 0)];
    let mut cheap = bucket("cheap", "gpt-4o", 0, 1);
    cheap.errors = 1;
    let mut input = read(vec![cheap, bucket("dear", "gpt-4o", 0, 5)]);
    input.latencies = vec![GroupLatency {
        key: "dear".into(),
        p50_nanos: 7,
        p95_nanos: 9,
    }];

    let spend = Spend::priced(input, &PriceBook::new(&prices), &query());

    let keys: Vec<&str> = spend.groups.iter().map(|group| &*group.key).collect();
    assert_eq!(keys, ["dear", "cheap"]);
    assert_eq!(
        (spend.groups[0].p50_nanos, spend.groups[0].p95_nanos),
        (7, 9)
    );
    assert_eq!(spend.groups[1].errors, 1);
}

#[test]
fn markers_are_ordered_by_time() {
    let marker = |time, version: &str| PromptMarker {
        time_unix_nanos: time,
        feature: "f".into(),
        prompt_version: version.into(),
    };
    let input = SpendRead {
        markers: vec![marker(9, "v2"), marker(3, "v1")],
        ..SpendRead::default()
    };

    let spend = Spend::priced(input, &PriceBook::new(&[]), &query());

    let versions: Vec<&str> = spend.markers.iter().map(|m| &*m.prompt_version).collect();
    assert_eq!(versions, ["v1", "v2"]);
}

#[test]
fn group_by_parses_the_five_dimensions_and_refuses_the_rest() {
    for name in ["feature", "model", "customer", "environment", "provider"] {
        assert!(GroupBy::try_from(name).is_ok());
    }
    assert!(GroupBy::try_from("user").is_err());
    assert_eq!(GroupBy::default(), GroupBy::Feature);
}
