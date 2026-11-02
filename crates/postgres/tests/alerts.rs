use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use ferristrace_application::registry::{RepoFor, Settle, UnitOfWork, domain};
use ferristrace_domain::{
    AccountId, AlertChannels, AlertError, AlertEvent, AlertEvents, AlertOutbox, AlertRules,
    AlertSeverity, AlertStates, AlertStatus, ChannelKind, ChannelName, Comparison, Condition,
    CulpritPod, Culprits, Definition, DefinitionParts, DeliveryStatus, Destination, Duty,
    Evaluator, EventId, Followers, Labels, NoDataPolicy, Notification, NotificationId, Observation,
    Paging, ProbeError, ProjectId, RuleDraft, RuleName, RuleState, SeriesState, Settlement, Signal,
    SignalProbe, TenantId, Tick, TraceMeasure, TraceSignal, UnixSeconds, WebhookSecret, Window,
};
use ferristrace_infra::{FixedClock, SecretSeal};
use ferristrace_postgres::{CONTROL_PLANE, Postgres};
use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool};
use uuid::Uuid;

#[macro_use]
mod backend;

const NOW: u64 = 1_700_000_040;
fn blamed() -> Culprits {
    Culprits::new(
        vec![
            CulpritPod {
                name: "checkout-1".into(),
                node: Some("node-a".into()),
                count: 7,
            },
            CulpritPod {
                name: "checkout-2".into(),
                node: None,
                count: 3,
            },
        ],
        9,
        vec!["node-a".into(), "node-b".into()],
        vec!["1.4.2".into()],
        &Labels::new(BTreeMap::from([("queue".into(), "mail".into())])),
    )
}

const KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

async fn control_plane(schema: &str, connections: u32) -> Option<PgPool> {
    let url = backend::url()?;
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("connecting to postgres");
    sqlx::query(AssertSqlSafe(format!(
        "DROP SCHEMA IF EXISTS {schema} CASCADE"
    )))
    .execute(&pool)
    .await
    .expect("dropping a leftover schema");
    sqlx::query(AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&pool)
        .await
        .expect("creating the test schema");
    pool.close().await;

    let owned = schema.to_owned();
    let scoped = PgPoolOptions::new()
        .max_connections(connections)
        .after_connect(move |connection, _| {
            let schema = owned.clone();
            Box::pin(async move {
                sqlx::query(AssertSqlSafe(format!("SET search_path TO {schema}")))
                    .execute(connection)
                    .await
                    .map(|_| ())
            })
        })
        .connect(&url)
        .await
        .expect("connecting to the test schema");
    CONTROL_PLANE
        .run(&scoped)
        .await
        .expect("the migrations apply");
    Some(scoped)
}

fn backend_of(pool: &PgPool) -> Postgres<FixedClock> {
    Postgres::new(pool.clone(), FixedClock::at(NOW))
        .with_seal(Some(SecretSeal::from_hex(KEY).expect("a key")))
}

async fn an_org(pool: &PgPool, name: &str, owner: &str) -> (TenantId, AccountId) {
    let account = Uuid::new_v4();
    sqlx::query("INSERT INTO accounts (id, email) VALUES ($1, $2)")
        .bind(account)
        .bind(owner)
        .execute(pool)
        .await
        .expect("inserting an account");
    let org = backend::memberships(&backend_of(pool))
        .found(name, owner)
        .await
        .expect("the organisation is founded");
    (org, AccountId::new(account))
}

fn checkout(channels: Vec<ferristrace_domain::ChannelId>) -> Definition {
    Definition::new(DefinitionParts {
        signal: Signal::Traces(TraceSignal {
            service: Some("checkout".into()),
            operation: None,
            environment: Some("prod".into()),
            measure: TraceMeasure::ErrorRate,
            placement: ferristrace_domain::Placement::default(),
        }),
        condition: Condition::new(Comparison::Above, 5.0).expect("a threshold"),
        window_secs: 300,
        pending_secs: 120,
        severity: AlertSeverity::Critical,
        no_data: NoDataPolicy::Ok,
        channels,
        page: None,
    })
    .expect("a definition")
}

fn draft(name: &str, project: Option<ProjectId>, definition: Definition) -> RuleDraft {
    RuleDraft {
        name: RuleName::try_from(name).expect("a name"),
        project,
        enabled: true,
        definition,
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn rules_channels_states_and_events_read_back_whole() {
    let Some(pool) = control_plane("alerts_round_trip", 2).await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "Acme", "owner@example.com").await;
    let (theirs, _) = an_org(&pool, "Other", "other@example.com").await;

    let (hook, secret) = committed!(pg, |channels: AlertChannels| {
        let secret = WebhookSecret::generate();
        let hook = channels
            .create(
                org,
                &ChannelName::try_from("pager").expect("a name"),
                &Destination::parse(ChannelKind::Webhook, "https://hooks.example.com/in")
                    .expect("a url"),
                Some(&secret),
                owner,
            )
            .await
            .expect("kept")
            .expect("fresh");
        let twice = channels
            .create(
                org,
                &ChannelName::try_from("pager").expect("a name"),
                &Destination::parse(ChannelKind::Email, "ops@example.com").expect("an address"),
                None,
                owner,
            )
            .await
            .expect("kept");
        assert!(
            twice.is_none(),
            "a channel name is unique in its organisation"
        );
        (hook, secret)
    });
    assert!(hook.has_secret);
    let stored: Vec<u8> = sqlx::query_scalar("SELECT secret_sealed FROM alert_channels")
        .fetch_one(&pool)
        .await
        .expect("a sealed secret");
    assert!(
        !String::from_utf8_lossy(&stored).contains(secret.exposed()),
        "the secret is sealed at rest"
    );

    let rule = committed!(pg, |rules: AlertRules, channels: AlertChannels| {
        let armed = channels
            .armed(org, hook.id)
            .await
            .expect("read")
            .expect("held");
        assert_eq!(armed.secret.as_ref(), Some(&secret));
        assert_eq!(channels.armed(theirs, hook.id).await.expect("read"), None);
        let rule = rules
            .create(
                org,
                &draft("Checkout errors", None, checkout(vec![hook.id])),
                owner,
            )
            .await
            .expect("kept")
            .expect("fresh");
        assert_eq!(
            rules.get(org, rule.id).await.expect("read"),
            Some(rule.clone())
        );
        assert_eq!(rules.get(theirs, rule.id).await.expect("read"), None);
        assert_eq!(rules.list(org).await.expect("read"), vec![rule.clone()]);
        assert!(
            rules
                .create(
                    org,
                    &draft("Checkout errors", None, checkout(Vec::new())),
                    owner
                )
                .await
                .expect("kept")
                .is_none()
        );
        let mut paused = rule.clone();
        paused.enabled = false;
        assert!(rules.update(&paused).await.expect("updated"));
        assert!(rules.enabled().await.expect("read").is_empty());
        assert!(rules.update(&rule).await.expect("updated"));
        assert_eq!(rules.enabled().await.expect("read"), vec![rule.clone()]);
        rule
    });
    assert_eq!(rule.created_ms, NOW * 1_000);

    let labelled = Labels::new(BTreeMap::from([("queue".into(), "mail".into())]));
    committed!(pg, |states: AlertStates, events: AlertEvents| {
        for (status, since) in [(AlertStatus::Pending, 1), (AlertStatus::Firing, 2)] {
            states
                .keep(
                    rule.id,
                    &SeriesState {
                        labels: labelled.clone(),
                        state: RuleState {
                            status,
                            value: Some(7.5),
                            since_ms: since,
                            evaluated_ms: 3,
                        },
                    },
                )
                .await
                .expect("kept");
        }
        let kept = states.of(rule.id).await.expect("read");
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].labels, labelled);
        assert_eq!(kept[0].state.status, AlertStatus::Firing);
        assert_eq!(kept[0].state.since_ms, 2);
        assert_eq!(states.of_org(org).await.expect("read").len(), 1);
        assert!(states.of_org(theirs).await.expect("read").is_empty());
        for at_ms in [10, 20, 30] {
            events
                .record(&AlertEvent {
                    id: EventId::fresh(),
                    rule: rule.id,
                    org,
                    labels: labelled.clone(),
                    from: AlertStatus::Ok,
                    to: AlertStatus::Firing,
                    value: None,
                    at_ms,
                    context: (at_ms == 20).then(blamed),
                })
                .await
                .expect("recorded");
        }
        let history = events
            .history(org, rule.id, 15, 40, 100)
            .await
            .expect("read");
        let read: Vec<u64> = history.iter().map(|event| event.at_ms).collect();
        assert_eq!(read, vec![30, 20]);
        assert_eq!(history[0].context, None);
        assert_eq!(history[1].context, Some(blamed()));
        assert_eq!(events.prune(15).await.expect("pruned"), 1);
        assert!(
            events
                .history(theirs, rule.id, 0, 40, 100)
                .await
                .expect("read")
                .is_empty()
        );
    });

    committed!(pg, |channels: AlertChannels, rules: AlertRules| {
        assert!(channels.delete(org, hook.id).await.expect("deleted"));
        let unlinked = rules.get(org, rule.id).await.expect("read").expect("held");
        assert!(
            unlinked.definition.channels().is_empty(),
            "a deleted channel leaves its rules"
        );
    });
}

async fn queued(pg: &Postgres<FixedClock>, pool: &PgPool, count: usize) -> TenantId {
    let (org, owner) = an_org(pool, "Acme", "owner@example.com").await;
    committed!(*pg, |channels: AlertChannels,
                     rules: AlertRules,
                     events: AlertEvents,
                     outbox: AlertOutbox| {
        let mail = channels
            .create(
                org,
                &ChannelName::try_from("ops").expect("a name"),
                &Destination::parse(ChannelKind::Email, "ops@example.com").expect("an address"),
                None,
                owner,
            )
            .await
            .expect("kept")
            .expect("fresh");
        let rule = rules
            .create(
                org,
                &draft("Checkout errors", None, checkout(vec![mail.id])),
                owner,
            )
            .await
            .expect("kept")
            .expect("fresh");
        for at in 0..count {
            let event = AlertEvent {
                id: EventId::fresh(),
                rule: rule.id,
                org,
                labels: Labels::none(),
                from: AlertStatus::Ok,
                to: AlertStatus::Firing,
                value: Some(9.0),
                at_ms: NOW * 1_000 + at as u64,
                context: Some(blamed()),
            };
            events.record(&event).await.expect("recorded");
            outbox
                .enqueue(&Notification {
                    id: NotificationId::fresh(),
                    org,
                    rule: rule.id,
                    event: event.id,
                    channel: mail.id,
                    attempt: 0,
                    next_at_ms: NOW * 1_000,
                    status: DeliveryStatus::Pending,
                    last_error: None,
                })
                .await
                .expect("queued");
        }
    });
    org
}

async fn claim_six(pg: &Postgres<FixedClock>) -> Vec<NotificationId> {
    UnitOfWork::transaction(
        pg,
        || (),
        Settle::OnSuccess,
        |tx| async move {
            let outbox = <Postgres<FixedClock> as RepoFor<domain::AlertOutbox>>::build(pg, &tx);
            let claimed = outbox.claim(NOW * 1_000, 6).await.expect("claimed");
            tokio::time::sleep(Duration::from_millis(300)).await;
            for held in &claimed {
                outbox
                    .settle(held.notification.id, 1, &Settlement::Sent)
                    .await
                    .expect("settled");
            }
            Ok::<_, ()>(
                claimed
                    .iter()
                    .map(|held| held.notification.id)
                    .collect::<Vec<_>>(),
            )
        },
    )
    .await
    .expect("committed")
}

#[tokio::test]
async fn a_notification_is_claimed_by_one_drain_only_and_settles() {
    let Some(pool) = control_plane("alerts_claims", 4).await else {
        return;
    };
    let pg = backend_of(&pool);
    queued(&pg, &pool, 10).await;

    let (one, other) = tokio::join!(claim_six(&pg), claim_six(&pg));

    let one: BTreeSet<_> = one.into_iter().collect();
    let other: BTreeSet<_> = other.into_iter().collect();
    assert!(one.is_disjoint(&other), "no notification is claimed twice");
    assert_eq!(one.len() + other.len(), 10);
    let sent: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM alert_outbox WHERE status = 'sent' AND attempt = 1",
    )
    .fetch_one(&pool)
    .await
    .expect("a count");
    assert_eq!(sent, 10);
    let left = committed!(pg, |outbox: AlertOutbox| outbox
        .claim(NOW * 1_000 + 60_000, 50)
        .await
        .expect("claimed"));
    assert!(left.is_empty());
}

#[tokio::test]
async fn a_retried_notification_waits_and_an_abandoned_one_is_never_claimed_again() {
    let Some(pool) = control_plane("alerts_retries", 2).await else {
        return;
    };
    let pg = backend_of(&pool);
    queued(&pg, &pool, 1).await;

    let first = committed!(pg, |outbox: AlertOutbox| {
        let claimed = outbox.claim(NOW * 1_000, 50).await.expect("claimed");
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].dispatch.name.as_ref(), "Checkout errors");
        assert_eq!(claimed[0].dispatch.severity, AlertSeverity::Critical);
        assert_eq!(claimed[0].dispatch.context, Some(blamed()));
        outbox
            .settle(
                claimed[0].notification.id,
                1,
                &Settlement::Retry {
                    next_at_ms: NOW * 1_000 + 60_000,
                    error: "refused".into(),
                },
            )
            .await
            .expect("settled");
        claimed[0].notification.id
    });
    let early = committed!(pg, |outbox: AlertOutbox| outbox
        .claim(NOW * 1_000 + 59_000, 50)
        .await
        .expect("claimed"));
    let due = committed!(pg, |outbox: AlertOutbox| {
        let claimed = outbox
            .claim(NOW * 1_000 + 60_000, 50)
            .await
            .expect("claimed");
        outbox
            .settle(
                first,
                5,
                &Settlement::Abandoned {
                    error: "refused".into(),
                },
            )
            .await
            .expect("settled");
        claimed
    });
    let after = committed!(pg, |outbox: AlertOutbox| outbox
        .claim(u64::MAX / 2, 50)
        .await
        .expect("claimed"));

    assert!(early.is_empty());
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].notification.attempt, 1);
    assert_eq!(due[0].notification.last_error.as_deref(), Some("refused"));
    assert!(after.is_empty());
    let (status, attempt): (String, i32) =
        sqlx::query_as("SELECT status, attempt FROM alert_outbox")
            .fetch_one(&pool)
            .await
            .expect("a row");
    assert_eq!((status.as_str(), attempt), ("abandoned", 5));
}

struct Slow;

impl SignalProbe for &Slow {
    async fn observe(
        &self,
        _org: TenantId,
        _project: Option<ProjectId>,
        _signal: &Signal,
        _window: Window,
    ) -> Result<Vec<Observation>, ProbeError> {
        tokio::time::sleep(Duration::from_millis(300)).await;
        Ok(vec![Observation {
            labels: Labels::none(),
            value: Some(9.0),
        }])
    }

    async fn culprits(
        &self,
        _org: TenantId,
        _project: Option<ProjectId>,
        _signal: &Signal,
        _window: Window,
        _labels: &Labels,
    ) -> Result<Culprits, ProbeError> {
        Ok(Culprits::default())
    }
}

async fn tick(pg: &Postgres<FixedClock>, at: u64) -> Tick {
    UnitOfWork::transaction(
        pg,
        || AlertError::StoreUnreachable,
        Settle::OnSuccess,
        |tx| async move {
            Evaluator::new(
                <Postgres<FixedClock> as RepoFor<domain::AlertRules>>::build(pg, &tx),
                <Postgres<FixedClock> as RepoFor<domain::AlertStates>>::build(pg, &tx),
                <Postgres<FixedClock> as RepoFor<domain::AlertEvents>>::build(pg, &tx),
                <Postgres<FixedClock> as RepoFor<domain::AlertOutbox>>::build(pg, &tx),
                <Postgres<FixedClock> as RepoFor<domain::AlertDuty>>::build(pg, &tx),
                &Slow,
                Followers::new(
                    <Postgres<FixedClock> as RepoFor<domain::Incidents>>::build(pg, &tx),
                    <Postgres<FixedClock> as RepoFor<domain::IncidentTimeline>>::build(pg, &tx),
                ),
                Paging::new(
                    <Postgres<FixedClock> as RepoFor<domain::Schedules>>::build(pg, &tx),
                    <Postgres<FixedClock> as RepoFor<domain::Pages>>::build(pg, &tx),
                ),
            )
            .tick(UnixSeconds::new(at))
            .await
        },
    )
    .await
    .expect("a tick")
}

#[tokio::test]
async fn spec_l2_3_two_replicas_never_evaluate_the_same_tick_twice() {
    let Some(pool) = control_plane("alerts_replicas", 4).await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "Acme", "owner@example.com").await;
    let mut immediate = checkout(Vec::new()).into_parts();
    immediate.pending_secs = 0;
    committed!(pg, |rules: AlertRules| rules
        .create(
            org,
            &draft(
                "Checkout errors",
                None,
                Definition::new(immediate).expect("a definition")
            ),
            owner
        )
        .await
        .expect("kept")
        .expect("fresh"));

    let (one, other) = tokio::join!(tick(&pg, NOW), tick(&pg, NOW));
    let later_same_minute = tick(&pg, NOW + 15).await;
    let next_minute = tick(&pg, NOW + 60).await;

    let mut outcomes = [one, other];
    outcomes.sort_by_key(|outcome| matches!(outcome, Tick::Skipped));
    assert!(
        matches!(
            outcomes[0],
            Tick::Evaluated {
                rules: 1,
                transitions: 1,
                ..
            }
        ),
        "{outcomes:?}"
    );
    assert_eq!(outcomes[1], Tick::Skipped, "the other replica skips");
    assert_eq!(later_same_minute, Tick::Skipped, "a slot is evaluated once");
    assert!(matches!(
        next_minute,
        Tick::Evaluated {
            rules: 1,
            transitions: 0,
            ..
        }
    ));
    let events: i64 = sqlx::query_scalar("SELECT count(*) FROM alert_events")
        .fetch_one(&pool)
        .await
        .expect("a count");
    assert_eq!(events, 1, "one change, one event");
    let slot: i64 = sqlx::query_scalar("SELECT slot FROM alert_duties WHERE duty = $1")
        .bind(Duty::Evaluate.as_str())
        .fetch_one(&pool)
        .await
        .expect("a slot");
    assert_eq!(u64::try_from(slot).expect("positive"), (NOW + 60) / 60);
}
