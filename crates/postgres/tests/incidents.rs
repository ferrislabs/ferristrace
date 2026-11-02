use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ferristrace_application::registry::{RepoFor, Settle, UnitOfWork, domain};
use ferristrace_domain::{
    AccountId, AlertError, AlertRules, AlertSeverity, AlertStatus, Attachment, AttachmentId,
    AttachmentKind, AttachmentLabel, Author, Change, CommentBody, Comparison, Condition,
    Definition, DefinitionParts, Evaluator, Followers, Incident, IncidentAttachments,
    IncidentFilter, IncidentId, IncidentScope, IncidentSeverity, IncidentStatus, IncidentTimeline,
    IncidentTitle, Incidents, KeyId, Labels, NoDataPolicy, Observation, ProbeError, ProjectId,
    ProjectName, Reference, RuleDraft, RuleId, RuleName, Signal, SignalProbe, TenantId, Tick,
    TimelineEntry, TimelineId, TraceMeasure, TraceSignal, UnixSeconds, Window,
};
use ferristrace_infra::FixedClock;
use ferristrace_postgres::{CONTROL_PLANE, Postgres};
use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool};
use uuid::Uuid;

#[macro_use]
mod backend;

const NOW: u64 = 1_700_000_040;

async fn control_plane(schema: &str) -> Option<PgPool> {
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
        .max_connections(2)
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
}

async fn an_org(pool: &PgPool, name: &str, owner: &str) -> (TenantId, AccountId) {
    let account = Uuid::new_v4();
    sqlx::query("INSERT INTO accounts (id, email, password_hash) VALUES ($1, $2, 'chosen')")
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

async fn a_rule(pg: &Postgres<FixedClock>, org: TenantId, by: AccountId) -> RuleId {
    committed!(*pg, |rules: AlertRules| rules
        .create(
            org,
            &RuleDraft {
                name: RuleName::try_from("Checkout errors").expect("a name"),
                project: None,
                enabled: true,
                definition: Definition::new(DefinitionParts {
                    signal: Signal::Traces(TraceSignal {
                        service: Some("checkout".into()),
                        operation: None,
                        environment: None,
                        measure: TraceMeasure::ErrorRate,
                    }),
                    condition: Condition::new(Comparison::Above, 5.0).expect("a threshold"),
                    window_secs: 300,
                    pending_secs: 0,
                    severity: AlertSeverity::Critical,
                    no_data: NoDataPolicy::Ok,
                    channels: Vec::new(),
                })
                .expect("a definition"),
            },
            by,
        )
        .await
        .expect("kept")
        .expect("fresh")
        .id)
}

fn an_incident(
    org: TenantId,
    project: Option<ProjectId>,
    alert: Option<RuleId>,
    opened_ms: u64,
) -> Incident {
    Incident {
        id: IncidentId::fresh(),
        org,
        project,
        title: IncidentTitle::try_from("Checkout down").expect("a title"),
        severity: IncidentSeverity::Sev2,
        status: IncidentStatus::Investigating,
        owner: None,
        public: false,
        opened_ms,
        resolved_ms: None,
        opened_from_alert: alert,
    }
}

fn entry(incident: &Incident, author: Author, change: Change, at_ms: u64) -> TimelineEntry {
    TimelineEntry {
        id: TimelineId::fresh(),
        incident: incident.id,
        org: incident.org,
        author,
        change,
        at_ms,
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn incidents_timelines_and_attachments_read_back_whole() {
    let Some(pool) = control_plane("incidents_round_trip").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "Acme", "owner@example.com").await;
    let rule = a_rule(&pg, org, owner).await;
    let project = backend::projects(&pg)
        .claim(org, &ProjectName::try_from("storefront").expect("a name"))
        .await
        .expect("a project");
    let linked = an_incident(org, Some(project), Some(rule), 1_000);
    let by_hand = an_incident(org, None, None, 2_000);
    let attachment = Attachment {
        id: AttachmentId::fresh(),
        incident: linked.id,
        reference: Reference::parse(AttachmentKind::Trace, "0af7651916cd43dd8448eb211c80319c")
            .expect("a trace"),
        label: AttachmentLabel::try_from("failing payment").expect("a label"),
        at_ms: 1_500,
    };
    let me = Author::Account(owner);
    let changes = vec![
        entry(
            &linked,
            me.clone(),
            Change::Opened {
                title: linked.title.clone(),
                severity: IncidentSeverity::Sev2,
                alert: Some(rule),
            },
            1_000,
        ),
        entry(
            &linked,
            me.clone(),
            Change::Status {
                from: IncidentStatus::Investigating,
                to: IncidentStatus::Resolved,
            },
            1_100,
        ),
        entry(
            &linked,
            me.clone(),
            Change::Owner {
                from: None,
                to: Some(owner),
            },
            1_100,
        ),
        entry(
            &linked,
            Author::Agent("triage".into()),
            Change::Comment {
                body: CommentBody::try_from("rolled back").expect("a body"),
            },
            1_050,
        ),
        entry(
            &linked,
            Author::ApiKey(KeyId::new(Uuid::from_u128(7))),
            Change::Attachment(attachment.clone()),
            1_500,
        ),
        entry(
            &linked,
            Author::System,
            Change::Alert {
                rule,
                from: AlertStatus::Firing,
                to: AlertStatus::Ok,
                value: Some(2.5),
                labels: Labels::new(BTreeMap::from([("region".into(), "eu".into())])),
            },
            1_600,
        ),
    ];
    let mut resolved = linked.clone();
    resolved.status = IncidentStatus::Resolved;
    resolved.resolved_ms = Some(1_100);
    resolved.owner = Some(owner);
    resolved.public = true;

    let (held, open, closed, all, scoped, timeline, attached, kept) = committed!(
        pg,
        |incidents: Incidents, timeline: IncidentTimeline, attachments: IncidentAttachments| {
            incidents.open(&linked).await.expect("opened");
            incidents.open(&by_hand).await.expect("opened");
            for change in &changes {
                timeline.append(change).await.expect("appended");
            }
            attachments
                .attach(org, &attachment)
                .await
                .expect("attached");
            let kept = incidents.keep(&resolved).await.expect("kept");
            let ids = |found: Vec<Incident>| -> Vec<IncidentId> {
                found.into_iter().map(|incident| incident.id).collect()
            };
            let listed = |scope, project| IncidentFilter { scope, project };
            (
                incidents.get(org, linked.id).await.expect("a read"),
                ids(incidents
                    .list(org, &listed(IncidentScope::Open, None))
                    .await
                    .expect("a read")),
                ids(incidents
                    .list(org, &listed(IncidentScope::Resolved, None))
                    .await
                    .expect("a read")),
                ids(incidents
                    .list(org, &listed(IncidentScope::All, None))
                    .await
                    .expect("a read")),
                ids(incidents
                    .list(org, &listed(IncidentScope::All, Some(project)))
                    .await
                    .expect("a read")),
                timeline.of(org, linked.id).await.expect("a read"),
                attachments.of(org, linked.id).await.expect("a read"),
                kept,
            )
        }
    );

    assert!(kept);
    assert_eq!(held, Some(resolved));
    assert_eq!(open, vec![by_hand.id]);
    assert_eq!(closed, vec![linked.id]);
    assert_eq!(all, vec![by_hand.id, linked.id], "newest first");
    assert_eq!(scoped, vec![linked.id]);
    assert_eq!(timeline, changes, "appended order, whatever the clock said");
    assert_eq!(attached, vec![attachment]);
}

#[tokio::test]
async fn one_organisation_never_sees_or_writes_another_organisations_incidents() {
    let Some(pool) = control_plane("incidents_isolation").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "Acme", "owner@example.com").await;
    let (theirs, stranger) = an_org(&pool, "Other", "other@example.com").await;
    let rule = a_rule(&pg, org, owner).await;
    let mine = an_incident(org, None, Some(rule), 1_000);
    let mut forged = entry(
        &mine,
        Author::Account(stranger),
        Change::Comment {
            body: CommentBody::try_from("not yours").expect("a body"),
        },
        1_100,
    );
    forged.org = theirs;
    let mut moved = mine.clone();
    moved.org = theirs;
    moved.title = IncidentTitle::try_from("Taken over").expect("a title");
    let smuggled = an_incident(theirs, None, Some(rule), 1_000);

    let (seen, listed, timeline, attached, kept, following) = committed!(
        pg,
        |incidents: Incidents, timeline: IncidentTimeline, attachments: IncidentAttachments| {
            incidents.open(&mine).await.expect("opened");
            incidents.open(&smuggled).await.expect("refused quietly");
            timeline.append(&forged).await.expect("refused quietly");
            attachments
                .attach(
                    theirs,
                    &Attachment {
                        id: AttachmentId::fresh(),
                        incident: mine.id,
                        reference: Reference::parse(AttachmentKind::Logs, "q=boom")
                            .expect("a query"),
                        label: AttachmentLabel::try_from("boom").expect("a label"),
                        at_ms: 1,
                    },
                )
                .await
                .expect("refused quietly");
            (
                incidents.get(theirs, mine.id).await.expect("a read"),
                incidents
                    .list(theirs, &IncidentFilter::default())
                    .await
                    .expect("a read"),
                timeline.of(org, mine.id).await.expect("a read"),
                attachments.of(org, mine.id).await.expect("a read"),
                incidents.keep(&moved).await.expect("a write"),
                incidents.following(theirs, rule).await.expect("a read"),
            )
        }
    );

    assert_eq!(seen, None);
    assert!(listed.is_empty(), "the alert of another org links nothing");
    assert!(timeline.is_empty());
    assert!(attached.is_empty());
    assert!(!kept);
    assert!(following.is_empty());
}

#[tokio::test]
async fn an_alert_is_followed_by_its_open_incidents_only() {
    let Some(pool) = control_plane("incidents_following").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "Acme", "owner@example.com").await;
    let rule = a_rule(&pg, org, owner).await;
    let other = committed!(pg, |rules: AlertRules| {
        let mut draft = rules
            .get(org, rule)
            .await
            .expect("a read")
            .expect("the rule");
        draft.name = RuleName::try_from("Other").expect("a name");
        rules
            .create(
                org,
                &RuleDraft {
                    name: draft.name,
                    project: None,
                    enabled: true,
                    definition: draft.definition,
                },
                owner,
            )
            .await
            .expect("kept")
            .expect("fresh")
            .id
    });
    let open = an_incident(org, None, Some(rule), 1_000);
    let mut resolved = an_incident(org, None, Some(rule), 2_000);
    resolved.status = IncidentStatus::Resolved;
    resolved.resolved_ms = Some(3_000);
    let elsewhere = an_incident(org, None, Some(other), 4_000);

    let following = committed!(pg, |incidents: Incidents| {
        for incident in [&open, &resolved, &elsewhere] {
            incidents.open(incident).await.expect("opened");
        }
        incidents.following(org, rule).await.expect("a read")
    });

    assert_eq!(following, vec![open.id]);
}

struct Firing;

impl SignalProbe for &Firing {
    async fn observe(
        &self,
        _org: TenantId,
        _project: Option<ProjectId>,
        _signal: &Signal,
        _window: Window,
    ) -> Result<Vec<Observation>, ProbeError> {
        Ok(vec![Observation {
            labels: Labels::none(),
            value: Some(9.0),
        }])
    }
}

async fn evaluated(pg: &Postgres<FixedClock>, tx: ferristrace_postgres::SharedTx) -> Tick {
    Evaluator::new(
        <Postgres<FixedClock> as RepoFor<domain::AlertRules>>::build(pg, &tx),
        <Postgres<FixedClock> as RepoFor<domain::AlertStates>>::build(pg, &tx),
        <Postgres<FixedClock> as RepoFor<domain::AlertEvents>>::build(pg, &tx),
        <Postgres<FixedClock> as RepoFor<domain::AlertOutbox>>::build(pg, &tx),
        <Postgres<FixedClock> as RepoFor<domain::AlertDuty>>::build(pg, &tx),
        &Firing,
        Followers::new(
            <Postgres<FixedClock> as RepoFor<domain::Incidents>>::build(pg, &tx),
            <Postgres<FixedClock> as RepoFor<domain::IncidentTimeline>>::build(pg, &tx),
        ),
    )
    .tick(UnixSeconds::new(NOW))
    .await
    .expect("a tick")
}

async fn counted(pool: &PgPool, table: &str) -> i64 {
    sqlx::query_scalar(AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(pool)
        .await
        .expect("a count")
}

#[tokio::test]
async fn an_alert_change_reaches_the_timeline_in_the_transaction_of_its_event() {
    let Some(pool) = control_plane("incidents_alert_link").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "Acme", "owner@example.com").await;
    let rule = a_rule(&pg, org, owner).await;
    let slow = committed!(pg, |rules: AlertRules| {
        let held = rules
            .get(org, rule)
            .await
            .expect("a read")
            .expect("the rule");
        let mut parts = held.definition.into_parts();
        parts.pending_secs = 120;
        rules
            .create(
                org,
                &RuleDraft {
                    name: RuleName::try_from("Slow checkout errors").expect("a name"),
                    project: None,
                    enabled: true,
                    definition: Definition::new(parts).expect("a definition"),
                },
                owner,
            )
            .await
            .expect("kept")
            .expect("fresh")
            .id
    });
    let incident = an_incident(org, None, Some(rule), 1_000);
    let pending = an_incident(org, None, Some(slow), 1_000);
    committed!(pg, |incidents: Incidents| {
        incidents.open(&incident).await.expect("opened");
        incidents.open(&pending).await.expect("opened");
    });

    let rehearsed = pg
        .scratch(|tx| evaluated(&pg, tx))
        .await
        .expect("a scratch transaction");
    let (events_after_rollback, entries_after_rollback) = (
        counted(&pool, "alert_events").await,
        counted(&pool, "incident_events").await,
    );
    let backend = &pg;
    let ticked = UnitOfWork::transaction(
        backend,
        || AlertError::StoreUnreachable,
        Settle::OnSuccess,
        |tx| async move { Ok::<_, AlertError>(evaluated(backend, tx).await) },
    )
    .await
    .expect("committed");
    let (timeline, quiet) = committed!(pg, |timeline: IncidentTimeline| (
        timeline.of(org, incident.id).await.expect("a read"),
        timeline.of(org, pending.id).await.expect("a read"),
    ));

    assert!(matches!(rehearsed, Tick::Evaluated { transitions: 2, .. }));
    assert_eq!((events_after_rollback, entries_after_rollback), (0, 0));
    assert!(matches!(ticked, Tick::Evaluated { transitions: 2, .. }));
    assert_eq!(counted(&pool, "alert_events").await, 2);
    assert!(
        quiet.is_empty(),
        "ok to pending is history, not incident news"
    );
    assert_eq!(timeline.len(), 1);
    assert_eq!(timeline[0].author, Author::System);
    assert_eq!(
        timeline[0].change,
        Change::Alert {
            rule,
            from: AlertStatus::Ok,
            to: AlertStatus::Firing,
            value: Some(9.0),
            labels: Labels::none(),
        }
    );
    assert_eq!(timeline[0].at_ms, NOW * 1_000);
}

fn sources_under(root: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(root).expect("a readable directory") {
        let path = entry.expect("an entry").path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name != "target") {
                sources_under(&path, found);
            }
        } else if path
            .extension()
            .is_some_and(|extension| extension == "rs" || extension == "sql")
        {
            found.push(path);
        }
    }
}

#[test]
fn no_statement_in_the_application_rewrites_an_incident_timeline_entry() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace");
    let mut sources = Vec::new();
    for root in ["crates", "apps"] {
        sources_under(&workspace.join(root), &mut sources);
    }
    let this_test = Path::new(file!())
        .file_name()
        .expect("this file")
        .to_owned();

    let offending: Vec<PathBuf> = sources
        .into_iter()
        .filter(|path| {
            !(path.file_name() == Some(this_test.as_os_str())
                && path
                    .parent()
                    .is_some_and(|parent| parent.ends_with("tests")))
        })
        .filter(|path| {
            let text = std::fs::read_to_string(path)
                .expect("a readable source")
                .to_lowercase()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            [
                "update incident_events",
                "delete from incident_events",
                "truncate incident_events",
            ]
            .iter()
            .any(|statement| text.contains(statement))
        })
        .collect();

    assert!(offending.is_empty(), "{offending:?}");
}
