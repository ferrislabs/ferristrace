use ferristrace_domain::{
    AccountId, AlertRules, AlertSeverity, Author, Change, Comparison, Condition,
    DEPLOY_WATCH_TICK_SECS, Definition, DefinitionParts, Deploy, DeployFeed, DeployMarks, Deployed,
    Followed, Incident, IncidentId, IncidentSeverity, IncidentStatus, IncidentTimeline,
    IncidentTitle, Incidents, NoDataPolicy, RuleDraft, RuleId, RuleName, Signal, StoreError,
    TenantId, TimelineEntry, TimelineId, TraceMeasure, TraceSignal, UnixSeconds, Watch, Window,
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

async fn a_rule(pg: &Postgres<FixedClock>, org: TenantId, by: AccountId, service: &str) -> RuleId {
    committed!(*pg, |rules: AlertRules| rules
        .create(
            org,
            &RuleDraft {
                name: RuleName::try_from("Errors").expect("a name"),
                project: None,
                enabled: true,
                definition: Definition::new(DefinitionParts {
                    signal: Signal::Traces(TraceSignal {
                        service: Some(service.into()),
                        operation: None,
                        environment: None,
                        measure: TraceMeasure::ErrorRate,
                        placement: ferristrace_domain::Placement::default(),
                    }),
                    condition: Condition::new(Comparison::Above, 5.0).expect("a threshold"),
                    window_secs: 300,
                    pending_secs: 0,
                    severity: AlertSeverity::Critical,
                    no_data: NoDataPolicy::Ok,
                    channels: Vec::new(),
                    page: None,
                    mentions: ferristrace_domain::Mentions::default(),
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

fn an_incident(org: TenantId, alert: Option<RuleId>, opened_ms: u64) -> Incident {
    Incident {
        id: IncidentId::fresh(),
        org,
        project: None,
        title: IncidentTitle::try_from("Checkout down").expect("a title"),
        severity: IncidentSeverity::Sev2,
        status: IncidentStatus::Investigating,
        owner: None,
        public: false,
        opened_ms,
        resolved_ms: None,
        opened_from_alert: alert,
        paged_schedule: None,
    }
}

fn deploy_entry(
    incident: &Incident,
    version: &str,
    environment: &str,
    at_ms: u64,
) -> TimelineEntry {
    TimelineEntry {
        id: TimelineId::fresh(),
        incident: incident.id,
        org: incident.org,
        author: Author::System,
        change: Change::Deploy {
            service: "checkout".into(),
            version: version.into(),
            environment: environment.into(),
        },
        at_ms,
    }
}

struct Feed(Vec<Deployed>);

impl DeployFeed for Feed {
    async fn deployed(&self, _window: Window) -> Result<Vec<Deployed>, StoreError> {
        Ok(self.0.clone())
    }
}

fn deployed(org: TenantId, service: &str, version: &str, at_ms: u64) -> Deployed {
    Deployed {
        org,
        deploy: Deploy {
            service: service.into(),
            version: version.into(),
            environment: "prod".into(),
            at_unix_nanos: at_ms * 1_000_000,
        },
    }
}

async fn deploy_entries(pool: &PgPool, incident: &Incident) -> Vec<(String, String, String)> {
    sqlx::query_as(
        "SELECT detail ->> 'service', detail ->> 'version', detail ->> 'environment' \
         FROM incident_events WHERE incident_id = $1 AND kind = 'deploy' ORDER BY seq",
    )
    .bind(incident.id.as_uuid())
    .fetch_all(pool)
    .await
    .expect("a read")
}

#[tokio::test]
async fn spec_l3_4_a_deploy_lands_once_per_incident_however_many_times_it_is_appended() {
    let Some(pool) = control_plane("deploys_exactly_once").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, _) = an_org(&pool, "Acme", "owner@example.com").await;
    let one = an_incident(org, None, 1_000);
    let two = an_incident(org, None, 1_000);
    let first = deploy_entry(&one, "2.0.0", "prod", 2_000);
    let again = deploy_entry(&one, "2.0.0", "prod", 2_500);
    let other_version = deploy_entry(&one, "2.0.1", "prod", 3_000);
    let other_environment = deploy_entry(&one, "2.0.0", "staging", 3_100);
    let elsewhere = deploy_entry(&two, "2.0.0", "prod", 2_000);

    let landed = committed!(pg, |incidents: Incidents, timeline: IncidentTimeline| {
        incidents.open(&one).await.expect("opened");
        incidents.open(&two).await.expect("opened");
        let mut landed = Vec::new();
        for entry in [
            &first,
            &again,
            &other_version,
            &other_environment,
            &elsewhere,
            &first,
        ] {
            landed.push(timeline.append_once(entry).await.expect("appended"));
        }
        landed
    });

    assert_eq!(landed, vec![true, false, true, true, true, false]);
    assert_eq!(
        deploy_entries(&pool, &one).await,
        vec![
            ("checkout".to_owned(), "2.0.0".to_owned(), "prod".to_owned()),
            ("checkout".to_owned(), "2.0.1".to_owned(), "prod".to_owned()),
            (
                "checkout".to_owned(),
                "2.0.0".to_owned(),
                "staging".to_owned()
            ),
        ]
    );
    assert_eq!(deploy_entries(&pool, &two).await.len(), 1);
}

#[tokio::test]
async fn the_database_itself_refuses_a_second_deploy_entry_for_the_same_incident_and_deploy() {
    let Some(pool) = control_plane("deploys_unique_index").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, _) = an_org(&pool, "Acme", "owner@example.com").await;
    let incident = an_incident(org, None, 1_000);
    let first = deploy_entry(&incident, "2.0.0", "prod", 2_000);
    let second = deploy_entry(&incident, "2.0.0", "prod", 2_001);
    committed!(pg, |incidents: Incidents, timeline: IncidentTimeline| {
        incidents.open(&incident).await.expect("opened");
        timeline.append(&first).await.expect("appended");
    });

    let refused = {
        let backend = &pg;
        ferristrace_application::registry::UnitOfWork::transaction(
            backend,
            || (),
            ferristrace_application::registry::Settle::OnSuccess,
            |tx| {
                let timeline = <_ as ferristrace_application::registry::RepoFor<
                    ferristrace_application::registry::domain::IncidentTimeline,
                >>::build(backend, &tx);
                async move { timeline.append(&second).await.map_err(|_| ()) }
            },
        )
        .await
    };

    assert!(refused.is_err());
    assert_eq!(deploy_entries(&pool, &incident).await.len(), 1);
}

#[tokio::test]
async fn two_replicas_appending_the_same_deploy_at_once_leave_one_entry() {
    let Some(pool) = control_plane("deploys_concurrent").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, _) = an_org(&pool, "Acme", "owner@example.com").await;
    let incident = an_incident(org, None, 1_000);
    committed!(pg, |incidents: Incidents| {
        incidents.open(&incident).await.expect("opened");
    });
    let left = deploy_entry(&incident, "2.0.0", "prod", 2_000);
    let right = deploy_entry(&incident, "2.0.0", "prod", 2_000);

    let (a, b) = tokio::join!(
        async {
            committed!(pg, |timeline: IncidentTimeline| timeline
                .append_once(&left)
                .await
                .expect("appended"))
        },
        async {
            committed!(pg, |timeline: IncidentTimeline| timeline
                .append_once(&right)
                .await
                .expect("appended"))
        },
    );

    assert_eq!(u8::from(a) + u8::from(b), 1);
    assert_eq!(deploy_entries(&pool, &incident).await.len(), 1);
}

#[tokio::test]
async fn the_watermark_only_moves_forward() {
    let Some(pool) = control_plane("deploys_watermark").await else {
        return;
    };
    let pg = backend_of(&pool);

    let (none, planted, kept, later) = committed!(pg, |marks: DeployMarks| {
        let none = marks.watermark().await.expect("a read");
        marks.advance(5_000).await.expect("advanced");
        let planted = marks.watermark().await.expect("a read");
        marks.advance(4_000).await.expect("advanced");
        let kept = marks.watermark().await.expect("a read");
        marks.advance(6_000).await.expect("advanced");
        (
            none,
            planted,
            kept,
            marks.watermark().await.expect("a read"),
        )
    });

    assert_eq!(none, None);
    assert_eq!(planted, Some(5_000));
    assert_eq!(kept, Some(5_000));
    assert_eq!(later, Some(6_000));
}

#[tokio::test]
async fn spec_l3_4_the_duty_appends_a_deploy_to_the_open_incidents_it_concerns_once() {
    let Some(pool) = control_plane("deploys_duty").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "Acme", "owner@example.com").await;
    let (theirs, _) = an_org(&pool, "Other", "other@example.com").await;
    let rule = a_rule(&pg, org, owner, "checkout").await;
    let now_ms = NOW * 1_000;
    let by_alert = an_incident(org, Some(rule), now_ms - 600_000);
    let by_hand = an_incident(org, None, now_ms - 600_000);
    let foreign = an_incident(theirs, None, now_ms - 600_000);
    committed!(pg, |incidents: Incidents, marks: DeployMarks| {
        incidents.open(&by_alert).await.expect("opened");
        incidents.open(&by_hand).await.expect("opened");
        incidents.open(&foreign).await.expect("opened");
        marks.advance(now_ms - 60_000).await.expect("planted");
    });
    let feed = Feed(vec![
        deployed(org, "checkout", "2.0.0", now_ms - 30_000),
        deployed(org, "mailer", "7", now_ms - 20_000),
    ]);

    let tick = |at: u64| {
        let feed = &feed;
        let pg = &pg;
        async move {
            committed!(*pg, |incidents: Incidents,
                             timeline: IncidentTimeline,
                             rules: AlertRules,
                             marks: DeployMarks,
                             duty: AlertDuty| {
                Watch::new(feed, incidents, timeline, rules, marks, duty)
                    .tick(UnixSeconds::new(at))
                    .await
                    .expect("a tick")
            })
        }
    };
    let first = tick(NOW).await;
    let second = tick(NOW + DEPLOY_WATCH_TICK_SECS).await;
    let same_slot = tick(NOW + DEPLOY_WATCH_TICK_SECS + 1).await;

    assert_eq!(
        first,
        Followed::Worked {
            deploys: 2,
            entries: 3
        }
    );
    assert_eq!(
        second,
        Followed::Worked {
            deploys: 2,
            entries: 0
        }
    );
    assert_eq!(same_slot, Followed::Skipped);
    assert_eq!(
        deploy_entries(&pool, &by_alert).await,
        vec![("checkout".to_owned(), "2.0.0".to_owned(), "prod".to_owned())]
    );
    assert_eq!(deploy_entries(&pool, &by_hand).await.len(), 2);
    assert!(deploy_entries(&pool, &foreign).await.is_empty());
}
