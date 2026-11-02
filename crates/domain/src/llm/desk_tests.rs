use std::sync::Mutex;

use super::desk::Llm;
use super::entities::{BucketTotals, Call, CallPage, PricedCall, SeenModel, SpendRead};
use super::errors::LlmError;
use super::ports::LlmReader;
use super::value_objects::{
    CallQuery, Cost, ModelPattern, PriceDraft, PriceId, Provider, Rates, SpendQuery,
};
use crate::access::key_ring_tests::{
    MINE, MemberOfMine, OTHER, OURS, OneSession, Ring, THEIRS, a_project, an_org, scopes, token,
};
use crate::access::{KeyScope, Reader, Readers};
use crate::audit::{Action, TargetKind};
use crate::catalogue::ProjectId;
use crate::explore::Window;
use crate::organisation::{OrgRole, Whoami};
use crate::store::StoreError;
use crate::telemetry::{SpanId, TenantId, TraceId};
use crate::testing::{Notebook, PriceLedger};

type Desk<'a> = Llm<OneSession, MemberOfMine, &'a Ring, PriceLedger, Notebook>;

#[derive(Default)]
struct Shelf {
    asked: Mutex<Vec<(TenantId, Option<ProjectId>)>>,
}

impl Shelf {
    fn asked(&self) -> Vec<(TenantId, Option<ProjectId>)> {
        self.asked.lock().expect("the lock").clone()
    }
}

fn a_call(model: &str, at_ms: u64) -> Call {
    Call {
        time_unix_nanos: at_ms * 1_000_000,
        trace: TraceId::try_from([1u8; 16].as_slice()).expect("a trace"),
        span: SpanId::try_from([1u8; 8].as_slice()).expect("a span"),
        service: "api".into(),
        provider: "openai".into(),
        operation: "chat".into(),
        request_model: model.into(),
        response_model: String::new().into(),
        input_tokens: 1_000_000,
        output_tokens: 0,
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

impl LlmReader for &Shelf {
    async fn calls(&self, org: TenantId, query: &CallQuery) -> Result<CallPage, StoreError> {
        self.asked
            .lock()
            .expect("the lock")
            .push((org, query.project));
        Ok(CallPage {
            calls: vec![
                PricedCall {
                    call: a_call("gpt-4o", 1_000),
                    cost: None,
                },
                PricedCall {
                    call: a_call("mystery", 1_000),
                    cost: None,
                },
            ],
            next: None,
        })
    }

    async fn spend(&self, org: TenantId, query: &SpendQuery) -> Result<SpendRead, StoreError> {
        self.asked
            .lock()
            .expect("the lock")
            .push((org, query.filter.project));
        Ok(SpendRead {
            buckets: vec![BucketTotals {
                key: "f".into(),
                provider: "openai".into(),
                request_model: "gpt-4o".into(),
                response_model: String::new().into(),
                slot: 0,
                calls: 1,
                errors: 0,
                input_tokens: 1_000_000,
                output_tokens: 0,
                cached_input_tokens: 0,
            }],
            ..SpendRead::default()
        })
    }

    async fn models(
        &self,
        org: TenantId,
        _window: Window,
        project: Option<ProjectId>,
    ) -> Result<Vec<SeenModel>, StoreError> {
        self.asked.lock().expect("the lock").push((org, project));
        Ok(vec![
            SeenModel {
                provider: "openai".into(),
                request_model: "gpt-4o".into(),
                response_model: "gpt-4o-2024-08-06".into(),
                calls: 3,
                last_unix_nanos: 1_000_000_000,
            },
            SeenModel {
                provider: "openai".into(),
                request_model: "mystery".into(),
                response_model: String::new().into(),
                calls: 2,
                last_unix_nanos: 1_000_000_000,
            },
            SeenModel {
                provider: "openai".into(),
                request_model: "other-request".into(),
                response_model: "mystery".into(),
                calls: 4,
                last_unix_nanos: 2_000_000_000,
            },
        ])
    }
}

fn desk<'a>(role: OrgRole, ledger: &PriceLedger, notebook: &Notebook, keys: &'a Ring) -> Desk<'a> {
    Llm::new(
        Readers::new(Whoami::new(OneSession, MemberOfMine(role)), keys),
        ledger.clone(),
        notebook.clone(),
    )
}

fn draft(model: &str, effective_from_ms: u64) -> PriceDraft {
    PriceDraft {
        provider: Provider::try_from("openai").expect("a provider"),
        model: ModelPattern::try_from(model).expect("a model"),
        rates: Rates::new(2_000_000, 8_000_000, None).expect("rates"),
        effective_from_ms,
    }
}

fn window() -> Window {
    Window::new(1, 2).expect("a window")
}

fn who() -> Reader {
    Reader::Session(token())
}

#[tokio::test]
async fn an_admin_adds_a_price_and_the_write_is_audited() {
    let (ledger, notebook, keys) = (PriceLedger::default(), Notebook::default(), Ring::default());

    let price = desk(OrgRole::Admin, &ledger, &notebook, &keys)
        .add(&token(), an_org(MINE), &draft("gpt-4o", 0))
        .await
        .expect("a price");

    assert_eq!(ledger.prices(an_org(MINE)), vec![price.clone()]);
    let entry = &notebook.entries()[0];
    assert_eq!(entry.action, Action::PriceCreated);
    assert_eq!(entry.target.kind, TargetKind::Price);
    assert_eq!(
        entry.target.id.as_deref(),
        Some(price.id.to_string().as_str())
    );
    assert_eq!(entry.detail.get("model"), Some(Some("gpt-4o")));
    assert_eq!(entry.detail.get("input"), Some(Some("2000000")));
}

#[tokio::test]
async fn a_member_cannot_change_a_price_but_reads_them() {
    let (ledger, notebook, keys) = (PriceLedger::default(), Notebook::default(), Ring::default());
    let member = desk(OrgRole::Member, &ledger, &notebook, &keys);

    let refused = member
        .add(&token(), an_org(MINE), &draft("gpt-4o", 0))
        .await;
    let deleted = member
        .remove(&token(), an_org(MINE), PriceId::fresh())
        .await;
    let listed = member.list(&who(), an_org(MINE)).await;

    assert_eq!(refused, Err(LlmError::NotYoursToChange));
    assert_eq!(deleted, Err(LlmError::NotYoursToChange));
    assert_eq!(listed, Ok(Vec::new()));
    assert!(notebook.entries().is_empty());
}

#[tokio::test]
async fn a_price_is_unique_per_provider_model_and_date() {
    let (ledger, notebook, keys) = (PriceLedger::default(), Notebook::default(), Ring::default());
    let admin = desk(OrgRole::Admin, &ledger, &notebook, &keys);
    admin
        .add(&token(), an_org(MINE), &draft("gpt-4o", 5))
        .await
        .expect("a price");

    let again = admin.add(&token(), an_org(MINE), &draft("gpt-4o", 5)).await;
    let later = admin.add(&token(), an_org(MINE), &draft("gpt-4o", 6)).await;

    assert_eq!(again, Err(LlmError::PriceTaken));
    assert!(later.is_ok());
    assert_eq!(notebook.entries().len(), 2);
}

#[tokio::test]
async fn deleting_a_price_audits_it_and_a_missing_one_is_unknown() {
    let (ledger, notebook, keys) = (PriceLedger::default(), Notebook::default(), Ring::default());
    let admin = desk(OrgRole::Admin, &ledger, &notebook, &keys);
    let price = admin
        .add(&token(), an_org(MINE), &draft("gpt-4o", 0))
        .await
        .expect("a price");

    admin
        .remove(&token(), an_org(MINE), price.id)
        .await
        .expect("a deletion");
    let missing = admin.remove(&token(), an_org(MINE), price.id).await;

    assert_eq!(missing, Err(LlmError::NoSuchPrice));
    assert!(ledger.prices(an_org(MINE)).is_empty());
    assert_eq!(notebook.entries()[1].action, Action::PriceDeleted);
}

#[tokio::test]
async fn an_admin_of_another_organisation_changes_nothing_here() {
    let (ledger, notebook, keys) = (PriceLedger::default(), Notebook::default(), Ring::default());

    let refused = desk(OrgRole::Admin, &ledger, &notebook, &keys)
        .add(&token(), an_org(THEIRS), &draft("gpt-4o", 0))
        .await;

    assert_eq!(refused, Err(LlmError::NotYours));
}

#[tokio::test]
async fn calls_carry_the_cost_of_the_price_in_force_and_null_without_one() {
    let (ledger, notebook, keys) = (PriceLedger::default(), Notebook::default(), Ring::default());
    let admin = desk(OrgRole::Admin, &ledger, &notebook, &keys);
    admin
        .add(&token(), an_org(MINE), &draft("gpt-4o", 0))
        .await
        .expect("a price");
    let shelf = Shelf::default();

    let page = admin
        .calls(&who(), an_org(MINE), &CallQuery::over(window()), &&shelf)
        .await
        .expect("a page");

    assert_eq!(
        page.calls[0].cost.map(Cost::picodollars),
        Some(2_000_000_000_000)
    );
    assert_eq!(page.calls[1].cost, None);
}

#[tokio::test]
async fn the_unpriced_models_are_the_seen_ones_no_price_reaches_with_their_call_counts() {
    let (ledger, notebook, keys) = (PriceLedger::default(), Notebook::default(), Ring::default());
    let admin = desk(OrgRole::Admin, &ledger, &notebook, &keys);
    admin
        .add(&token(), an_org(MINE), &draft("gpt-4o*", 0))
        .await
        .expect("a price");
    let shelf = Shelf::default();

    let unpriced = admin
        .unpriced(&who(), an_org(MINE), window(), &&shelf)
        .await
        .expect("a list");

    assert_eq!(unpriced.len(), 1);
    assert_eq!(&*unpriced[0].model, "mystery");
    assert_eq!(&*unpriced[0].provider, "openai");
    assert_eq!(unpriced[0].calls, 6);
    assert_eq!(unpriced[0].last_unix_nanos, 2_000_000_000);
}

#[tokio::test]
async fn a_project_bound_key_narrows_the_reads_and_cannot_widen_them() {
    let (ledger, notebook, keys) = (PriceLedger::default(), Notebook::default(), Ring::default());
    let desk = desk(OrgRole::Member, &ledger, &notebook, &keys);
    let bound = Reader::Key(keys.holding(
        an_org(MINE),
        Some(a_project(OURS)),
        scopes(&[KeyScope::Read]),
    ));
    let shelf = Shelf::default();

    desk.calls(&bound, an_org(MINE), &CallQuery::over(window()), &&shelf)
        .await
        .expect("a page");
    desk.unpriced(&bound, an_org(MINE), window(), &&shelf)
        .await
        .expect("a list");
    let mut elsewhere = CallQuery::over(window());
    elsewhere.project = Some(a_project(OTHER));
    let refused = desk.calls(&bound, an_org(MINE), &elsewhere, &&shelf).await;
    desk.spend(&bound, an_org(MINE), &SpendQuery::over(window()), &&shelf)
        .await
        .expect("a spend");
    let mut spend_elsewhere = SpendQuery::over(window());
    spend_elsewhere.filter.project = Some(a_project(OTHER));
    let spend_refused = desk
        .spend(&bound, an_org(MINE), &spend_elsewhere, &&shelf)
        .await;

    assert_eq!(
        shelf.asked(),
        vec![
            (an_org(MINE), Some(a_project(OURS))),
            (an_org(MINE), Some(a_project(OURS))),
            (an_org(MINE), Some(a_project(OURS)))
        ]
    );
    assert_eq!(refused, Err(LlmError::NotYours));
    assert_eq!(spend_refused, Err(LlmError::NotYours));
}

#[tokio::test]
async fn a_key_of_another_organisation_reads_nothing() {
    let (ledger, notebook, keys) = (PriceLedger::default(), Notebook::default(), Ring::default());
    let other = Reader::Key(keys.holding(an_org(THEIRS), None, scopes(&[KeyScope::Read])));
    let shelf = Shelf::default();

    let refused = desk(OrgRole::Member, &ledger, &notebook, &keys)
        .calls(&other, an_org(MINE), &CallQuery::over(window()), &&shelf)
        .await;

    assert_eq!(refused, Err(LlmError::NotYours));
    assert!(shelf.asked().is_empty());
}

#[tokio::test]
async fn spend_is_priced_with_the_organisations_prices() {
    let (ledger, notebook, keys) = (PriceLedger::default(), Notebook::default(), Ring::default());
    let admin = desk(OrgRole::Admin, &ledger, &notebook, &keys);
    admin
        .add(&token(), an_org(MINE), &draft("gpt-4o", 0))
        .await
        .expect("a price");
    let shelf = Shelf::default();

    let spend = admin
        .spend(&who(), an_org(MINE), &SpendQuery::over(window()), &&shelf)
        .await
        .expect("a spend");

    assert_eq!(spend.groups[0].cost.picodollars(), 2_000_000_000_000);
    assert_eq!(spend.groups[0].unpriced, 0);
}
