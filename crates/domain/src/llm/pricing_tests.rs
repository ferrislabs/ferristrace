use uuid::Uuid;

use super::entities::{Call, Price};
use super::pricing::PriceBook;
use super::value_objects::{Cost, ModelPattern, PriceId, Provider, Rates};
use crate::telemetry::{SpanId, TraceId};

const DAY_MS: u64 = 86_400_000;

fn price(provider: &str, model: &str, effective_from_ms: u64, input: u64, output: u64) -> Price {
    Price {
        id: PriceId::new(Uuid::from_u128(
            u128::from(effective_from_ms) + u128::from(input),
        )),
        provider: Provider::try_from(provider).expect("a provider"),
        model: ModelPattern::try_from(model).expect("a model"),
        rates: Rates::new(input, output, None).expect("rates"),
        effective_from_ms,
    }
}

fn cached(mut price: Price, rate: u64) -> Price {
    price.rates.cached_input = Some(rate);
    price
}

fn call(provider: &str, request: &str, response: &str, at_ms: u64) -> Call {
    Call {
        time_unix_nanos: at_ms * 1_000_000,
        trace: TraceId::try_from([1u8; 16].as_slice()).expect("a trace"),
        span: SpanId::try_from([1u8; 8].as_slice()).expect("a span"),
        service: "api".into(),
        provider: provider.into(),
        operation: "chat".into(),
        request_model: request.into(),
        response_model: response.into(),
        input_tokens: 1_000_000,
        output_tokens: 500_000,
        cached_input_tokens: 0,
        duration_nanos: 1,
        error: false,
        feature: "f".into(),
        customer: String::new().into(),
        user: String::new().into(),
        prompt_version: String::new().into(),
        environment: String::new().into(),
    }
}

#[test]
fn a_call_costs_its_tokens_times_the_price_per_million() {
    let prices = [price("openai", "gpt-4o", 0, 2_500_000, 10_000_000)];

    let cost = PriceBook::new(&prices).cost(&call("openai", "gpt-4o", "", DAY_MS));

    let cost = cost.expect("a cost");
    assert_eq!(cost.picodollars(), 7_500_000_000_000);
    assert!((cost.dollars() - 7.5).abs() < 1e-9);
}

#[test]
fn a_model_without_a_price_has_no_cost_rather_than_a_free_one() {
    let prices = [price("openai", "gpt-4o", 0, 1, 1)];

    let book = PriceBook::new(&prices);

    assert_eq!(book.cost(&call("openai", "gpt-5", "", DAY_MS)), None);
    assert_eq!(book.cost(&call("anthropic", "gpt-4o", "", DAY_MS)), None);
    assert_eq!(
        PriceBook::new(&[]).cost(&call("openai", "gpt-4o", "", 1)),
        None
    );
}

#[test]
fn the_price_in_force_at_the_call_time_applies() {
    let prices = [
        price("openai", "gpt-4o", 0, 1_000_000, 0),
        price("openai", "gpt-4o", 10 * DAY_MS, 3_000_000, 0),
    ];
    let book = PriceBook::new(&prices);

    let before = book.cost(&call("openai", "gpt-4o", "", 9 * DAY_MS));
    let at = book.cost(&call("openai", "gpt-4o", "", 10 * DAY_MS));
    let after = book.cost(&call("openai", "gpt-4o", "", 11 * DAY_MS));

    assert_eq!(before.map(Cost::picodollars), Some(1_000_000_000_000));
    assert_eq!(at.map(Cost::picodollars), Some(3_000_000_000_000));
    assert_eq!(after.map(Cost::picodollars), Some(3_000_000_000_000));
}

#[test]
fn a_price_that_starts_later_leaves_earlier_calls_unpriced() {
    let prices = [price("openai", "gpt-4o", 10 * DAY_MS, 1, 1)];

    let early = PriceBook::new(&prices).cost(&call("openai", "gpt-4o", "", 9 * DAY_MS));

    assert_eq!(early, None);
}

#[test]
fn an_exact_model_beats_a_prefix_whatever_the_dates() {
    let prices = [
        price("openai", "gpt-4o*", 5 * DAY_MS, 9_000_000, 0),
        price("openai", "gpt-4o-mini", 0, 150_000, 0),
    ];

    let found = PriceBook::new(&prices).find("openai", "gpt-4o-mini", 10 * DAY_MS);

    assert_eq!(found.map(|price| price.rates.input), Some(150_000));
}

#[test]
fn the_longest_prefix_wins() {
    let prices = [
        price("openai", "gpt*", 0, 1, 0),
        price("openai", "gpt-4o*", 0, 2, 0),
        price("openai", "gpt-4*", 0, 3, 0),
    ];

    let found = PriceBook::new(&prices).find("openai", "gpt-4o-2024-08-06", 1);

    assert_eq!(found.map(|price| price.rates.input), Some(2));
}

#[test]
fn a_prefix_does_not_match_a_model_that_merely_contains_it() {
    let prices = [price("openai", "gpt-4*", 0, 1, 0)];

    let found = PriceBook::new(&prices).find("openai", "my-gpt-4", 1);

    assert_eq!(found, None);
}

#[test]
fn the_latest_date_in_force_wins_among_equal_prefixes() {
    let prices = [
        price("openai", "gpt*", 0, 1, 0),
        price("openai", "gpt*", 5 * DAY_MS, 2, 0),
        price("openai", "gpt*", 20 * DAY_MS, 3, 0),
    ];

    let found = PriceBook::new(&prices).find("openai", "gpt-4", 10 * DAY_MS);

    assert_eq!(found.map(|price| price.rates.input), Some(2));
}

#[test]
fn the_response_model_is_priced_before_the_request_model() {
    let prices = [
        price("openai", "gpt-4o", 0, 1_000_000, 0),
        price("openai", "gpt-4o-2024-08-06", 0, 2_000_000, 0),
    ];
    let book = PriceBook::new(&prices);

    let answered = book.cost(&call("openai", "gpt-4o", "gpt-4o-2024-08-06", 1));
    let unanswered = book.cost(&call("openai", "gpt-4o", "", 1));
    let unknown_answer = book.cost(&call("openai", "gpt-4o", "gpt-unknown", 1));

    assert_eq!(answered.map(Cost::picodollars), Some(2_000_000_000_000));
    assert_eq!(unanswered.map(Cost::picodollars), Some(1_000_000_000_000));
    assert_eq!(
        unknown_answer.map(Cost::picodollars),
        Some(1_000_000_000_000)
    );
}

#[test]
fn the_provider_is_compared_without_regard_to_case() {
    let prices = [price("OpenAI", "gpt-4o", 0, 1, 0)];

    assert!(
        PriceBook::new(&prices)
            .find("openai", "gpt-4o", 1)
            .is_some()
    );
    assert!(
        PriceBook::new(&prices)
            .find("OPENAI", "gpt-4o", 1)
            .is_some()
    );
}

#[test]
fn cached_input_tokens_are_a_subset_of_the_input_tokens() {
    let prices = [cached(price("openai", "gpt-4o", 0, 2_000_000, 0), 500_000)];
    let mut hit = call("openai", "gpt-4o", "", 1);
    hit.input_tokens = 1_000_000;
    hit.cached_input_tokens = 400_000;
    hit.output_tokens = 0;

    let cost = PriceBook::new(&prices).cost(&hit).expect("a cost");

    assert_eq!(cost.picodollars(), 600_000 * 2_000_000 + 400_000 * 500_000);
}

#[test]
fn cached_tokens_cost_the_input_price_when_no_cached_price_is_set() {
    let prices = [price("openai", "gpt-4o", 0, 2_000_000, 0)];
    let mut hit = call("openai", "gpt-4o", "", 1);
    hit.input_tokens = 1_000_000;
    hit.cached_input_tokens = 400_000;
    hit.output_tokens = 0;

    let cost = PriceBook::new(&prices).cost(&hit).expect("a cost");

    assert_eq!(cost.picodollars(), 1_000_000 * 2_000_000);
}

#[test]
fn more_cached_tokens_than_input_tokens_never_underflows() {
    let prices = [cached(price("openai", "gpt-4o", 0, 2_000_000, 0), 500_000)];
    let mut odd = call("openai", "gpt-4o", "", 1);
    odd.input_tokens = 100;
    odd.cached_input_tokens = 300;
    odd.output_tokens = 0;

    let cost = PriceBook::new(&prices).cost(&odd).expect("a cost");

    assert_eq!(cost.picodollars(), 100 * 500_000);
}

#[test]
fn a_call_without_tokens_costs_nothing_but_is_priced() {
    let prices = [price("openai", "gpt-4o", 0, 2_000_000, 8_000_000)];
    let mut empty = call("openai", "gpt-4o", "", 1);
    empty.input_tokens = 0;
    empty.output_tokens = 0;

    let cost = PriceBook::new(&prices).cost(&empty);

    assert_eq!(cost.map(Cost::picodollars), Some(0));
}
