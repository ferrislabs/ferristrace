use ferristrace_domain::{
    AccountId, AlertRules, AlertSeverity, AlertStatus, BucketTotals, CallPage, CallQuery,
    Comparison, Condition, Definition, DefinitionParts, Evaluator, Followers, GroupBy,
    GroupLatency, Labels, LlmGroup, LlmMeasure, LlmReader, LlmSignal, ModelPattern, ModelPrices,
    NoDataPolicy, Paging, PriceDraft, Provider, Rates, RuleDraft, RuleName, SeenModel, Signal,
    SpendQuery, SpendRead, StoreError, TenantId, Tick, UnixSeconds, Window,
};
use std::collections::BTreeMap;
use uuid::Uuid;

use crate::SignalReaders;
use crate::testing::{AlertLedger, IncidentLedger, Inert, OncallLedger, PriceLedger};

const NOW: u64 = 1_700_003_600;
const SECOND: u64 = 1_000_000_000;
const MILLION: u64 = 1_000_000;

fn org() -> TenantId {
    TenantId::new(Uuid::from_u128(1))
}

fn by() -> AccountId {
    AccountId::new(Uuid::from_u128(9))
}

#[derive(Debug, Clone)]
struct Stored {
    feature: &'static str,
    model: &'static str,
    at_secs: u64,
    input_tokens: u64,
}

#[derive(Debug, Default)]
struct Shelf {
    calls: Vec<Stored>,
}

impl Shelf {
    fn with(
        mut self,
        feature: &'static str,
        model: &'static str,
        at_secs: u64,
        calls: u64,
    ) -> Self {
        for _ in 0..calls {
            self.calls.push(Stored {
                feature,
                model,
                at_secs,
                input_tokens: MILLION,
            });
        }
        self
    }
}

impl LlmReader for Shelf {
    async fn calls(&self, _org: TenantId, _query: &CallQuery) -> Result<CallPage, StoreError> {
        Ok(CallPage::default())
    }

    async fn spend(&self, org_: TenantId, query: &SpendQuery) -> Result<SpendRead, StoreError> {
        assert_eq!(org_, org());
        let window = query.filter.window;
        let mut buckets: BTreeMap<(Box<str>, &str), BucketTotals> = BTreeMap::new();
        for call in &self.calls {
            let nanos = call.at_secs * SECOND;
            let wanted = |filter: &Option<Box<str>>, held: &str| {
                filter.as_deref().is_none_or(|asked| asked == held)
            };
            if nanos < window.from_unix_nanos()
                || nanos >= window.to_unix_nanos()
                || !wanted(&query.filter.feature, call.feature)
                || !wanted(&query.filter.model, call.model)
            {
                continue;
            }
            let key: Box<str> = match query.group_by {
                GroupBy::Feature => call.feature.into(),
                GroupBy::Model => call.model.into(),
                _ => "all".into(),
            };
            let bucket = buckets
                .entry((key.clone(), call.model))
                .or_insert_with(|| BucketTotals {
                    key,
                    provider: "openai".into(),
                    request_model: call.model.into(),
                    response_model: "".into(),
                    slot: 0,
                    calls: 0,
                    errors: 0,
                    input_tokens: 0,
                    output_tokens: 0,
                    cached_input_tokens: 0,
                });
            bucket.calls += 1;
            bucket.input_tokens += call.input_tokens;
        }
        let latencies = buckets
            .keys()
            .map(|(key, _)| GroupLatency {
                key: key.clone(),
                p50_nanos: 100_000_000,
                p95_nanos: 900_000_000,
            })
            .collect();
        Ok(SpendRead {
            buckets: buckets.into_values().collect(),
            latencies,
            markers: Vec::new(),
        })
    }

    async fn models(
        &self,
        _org: TenantId,
        _window: Window,
        _project: Option<ferristrace_domain::ProjectId>,
    ) -> Result<Vec<SeenModel>, StoreError> {
        Ok(Vec::new())
    }
}

async fn priced() -> PriceLedger {
    let prices = PriceLedger::default();
    prices
        .create(
            org(),
            &PriceDraft {
                provider: Provider::try_from("openai").expect("a provider"),
                model: ModelPattern::try_from("gpt-4o").expect("a model"),
                rates: Rates::new(2_000_000, 0, None).expect("rates"),
                effective_from_ms: 0,
            },
            by(),
        )
        .await
        .expect("kept")
        .expect("fresh");
    prices
}

fn signal(measure: LlmMeasure, group_by: LlmGroup, feature: Option<&str>) -> LlmSignal {
    LlmSignal {
        measure,
        group_by,
        feature: feature.map(Into::into),
        customer: None,
        model: None,
        project: None,
    }
}

async fn rule(ledger: &AlertLedger, signal: LlmSignal, threshold: f64, no_data: NoDataPolicy) {
    AlertRules::create(
        ledger,
        org(),
        &RuleDraft {
            name: RuleName::try_from("ticket spend").expect("a name"),
            project: None,
            enabled: true,
            definition: Definition::new(DefinitionParts {
                signal: Signal::Llm(signal),
                condition: Condition::new(Comparison::Above, threshold).expect("a threshold"),
                window_secs: 3_600,
                pending_secs: 0,
                severity: AlertSeverity::Critical,
                no_data,
                channels: Vec::new(),
                page: None,
            })
            .expect("a definition"),
        },
        by(),
    )
    .await
    .expect("kept")
    .expect("fresh");
}

async fn ticked(
    ledger: &AlertLedger,
    shelf: Option<&Shelf>,
    prices: &PriceLedger,
    at: u64,
) -> Tick {
    let probe = SignalReaders::<Inert, Inert, Inert, Shelf, PriceLedger>::new(
        None, None, None, shelf, prices,
    );
    Evaluator::new(
        ledger.clone(),
        ledger.clone(),
        ledger.clone(),
        ledger.clone(),
        ledger.clone(),
        probe,
        Followers::new(IncidentLedger::default(), IncidentLedger::default()),
        Paging::new(OncallLedger::default(), OncallLedger::default()),
    )
    .tick(UnixSeconds::new(at))
    .await
    .expect("a tick")
}

fn statuses(ledger: &AlertLedger) -> Vec<(Labels, AlertStatus)> {
    ledger
        .states()
        .into_iter()
        .map(|(_, series)| (series.labels, series.state.status))
        .collect()
}

#[tokio::test]
async fn spec_l4_4_the_cost_of_a_feature_goes_ok_then_firing_as_priced_calls_cross_ten_dollars() {
    let prices = priced().await;
    let ledger = AlertLedger::default();
    rule(
        &ledger,
        signal(LlmMeasure::Cost, LlmGroup::None, Some("ticket-summary")),
        10.0,
        NoDataPolicy::Ok,
    )
    .await;
    let early = Shelf::default()
        .with("ticket-summary", "gpt-4o", NOW - 1_800, 3)
        .with("ticket-summary", "mystery", NOW - 1_800, 40)
        .with("other", "gpt-4o", NOW - 1_800, 50);

    let tick = ticked(&ledger, Some(&early), &prices, NOW).await;

    assert!(
        matches!(
            tick,
            Tick::Evaluated {
                rules: 1,
                failed: 0,
                ..
            }
        ),
        "{tick:?}"
    );
    assert_eq!(statuses(&ledger), vec![(Labels::none(), AlertStatus::Ok)]);

    let later = early.with("ticket-summary", "gpt-4o", NOW - 600, 3);
    ticked(&ledger, Some(&later), &prices, NOW + 60).await;

    assert_eq!(
        statuses(&ledger),
        vec![(Labels::none(), AlertStatus::Firing)]
    );
    let events = ledger.events();
    let firing = events.last().expect("an event");
    assert_eq!(firing.to, AlertStatus::Firing);
    assert!((firing.value.expect("a value") - 12.0).abs() < 1e-9);
}

#[tokio::test]
async fn a_grouped_llm_rule_keeps_one_state_per_group_and_names_the_group_that_fired() {
    let prices = priced().await;
    let ledger = AlertLedger::default();
    rule(
        &ledger,
        signal(LlmMeasure::Cost, LlmGroup::Feature, None),
        10.0,
        NoDataPolicy::Ok,
    )
    .await;
    let shelf = Shelf::default()
        .with("ticket-summary", "gpt-4o", NOW - 600, 6)
        .with("search", "gpt-4o", NOW - 600, 2);

    ticked(&ledger, Some(&shelf), &prices, NOW).await;

    let named = |feature: &str| Labels::new(BTreeMap::from([("feature".into(), feature.into())]));
    let mut held = statuses(&ledger);
    held.sort_by(|left, right| left.0.cmp(&right.0));
    assert_eq!(
        held,
        vec![
            (named("search"), AlertStatus::Ok),
            (named("ticket-summary"), AlertStatus::Firing),
        ]
    );
    let events = ledger.events();
    let fired = events
        .iter()
        .find(|event| event.to == AlertStatus::Firing)
        .expect("a firing event");
    assert_eq!(fired.labels, named("ticket-summary"));
    let context = fired.context.as_ref().expect("a context");
    assert_eq!(context.labels(), &named("ticket-summary"));
    assert!(context.pods().is_empty());
}

#[tokio::test]
async fn the_other_measures_read_tokens_calls_and_p95_in_milliseconds() {
    let prices = priced().await;
    let shelf = Shelf::default().with("a", "gpt-4o", NOW - 600, 4);
    for (measure, below, above) in [
        (LlmMeasure::Tokens, 3_999_999.0, 4_000_000.0),
        (LlmMeasure::Calls, 3.0, 4.0),
        (LlmMeasure::P95, 899.0, 900.0),
    ] {
        let ledger = AlertLedger::default();
        rule(
            &ledger,
            signal(measure, LlmGroup::None, None),
            below,
            NoDataPolicy::Ok,
        )
        .await;
        ticked(&ledger, Some(&shelf), &prices, NOW).await;
        assert_eq!(
            statuses(&ledger),
            vec![(Labels::none(), AlertStatus::Firing)],
            "{measure} above {below}"
        );
        let calm = AlertLedger::default();
        rule(
            &calm,
            signal(measure, LlmGroup::None, None),
            above,
            NoDataPolicy::Ok,
        )
        .await;
        ticked(&calm, Some(&shelf), &prices, NOW).await;
        assert_eq!(
            statuses(&calm),
            vec![(Labels::none(), AlertStatus::Ok)],
            "{measure} above {above}"
        );
    }
}

#[tokio::test]
async fn a_window_without_calls_is_no_data() {
    let prices = priced().await;
    let ledger = AlertLedger::default();
    rule(
        &ledger,
        signal(LlmMeasure::Cost, LlmGroup::None, Some("ticket-summary")),
        10.0,
        NoDataPolicy::NoData,
    )
    .await;
    let shelf = Shelf::default().with("ticket-summary", "gpt-4o", NOW - 7_200, 9);

    ticked(&ledger, Some(&shelf), &prices, NOW).await;

    assert_eq!(
        statuses(&ledger),
        vec![(Labels::none(), AlertStatus::NoData)]
    );
}

#[tokio::test]
async fn a_deployment_without_a_call_store_keeps_the_rule_unread() {
    let prices = priced().await;
    let ledger = AlertLedger::default();
    rule(
        &ledger,
        signal(LlmMeasure::Cost, LlmGroup::None, None),
        10.0,
        NoDataPolicy::Ok,
    )
    .await;

    let tick = ticked(&ledger, None, &prices, NOW).await;

    assert!(
        matches!(
            tick,
            Tick::Evaluated {
                rules: 1,
                failed: 1,
                ..
            }
        ),
        "{tick:?}"
    );
    assert!(statuses(&ledger).is_empty());
}
