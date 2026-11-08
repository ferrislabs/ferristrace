use ferristrace_application::registry::{RepoFor, Settle, UnitOfWork, domain};
use ferristrace_domain::{
    AccountId, Action, Actor, AlertRules, AlertSeverity, AuditEntry, AuditFilter, AuditTrail,
    ClaimedNotice, Comparison, Condition, Day, Definition, DefinitionParts, DeliveryStatus, Detail,
    EventId, Handoff, IncidentId, Named, NoDataPolicy, Notice, NoticeId, OncallError, OrgRole,
    Origin, Override, OverrideId, Page, PageId, PageScope, PageSource, PageState, Paged, Pager,
    Pages, Paging, Rotation, RotationLength, RuleDraft, RuleId, RuleName, Schedule, ScheduleDraft,
    ScheduleId, ScheduleName, Schedules, Settlement, Signal, Squads, Target, TargetKind, TeamId,
    TeamName, TenantId, TimeOfDay, Timezone, TraceMeasure, TraceSignal, UnixSeconds, Weekday,
    testing::Couriered,
};
use ferristrace_domain::{
    Author, Change, Incident, IncidentSeverity, IncidentStatus, IncidentTimeline, IncidentTitle,
    Incidents, PageSize, TimelineEntry, TimelineId,
};
use ferristrace_infra::FixedClock;
use ferristrace_postgres::{CONTROL_PLANE, Postgres};
use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool};
use uuid::Uuid;

#[macro_use]
mod backend;

const NOW: u64 = 1_793_613_600;

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
}

async fn an_account(pool: &PgPool, email: &str) -> AccountId {
    let account = Uuid::new_v4();
    sqlx::query("INSERT INTO accounts (id, email) VALUES ($1, $2)")
        .bind(account)
        .bind(email)
        .execute(pool)
        .await
        .expect("inserting an account");
    AccountId::new(account)
}

struct Crew {
    org: TenantId,
    owner: AccountId,
    ada: AccountId,
    bob: AccountId,
    team: TeamId,
}

async fn a_crew(pool: &PgPool, name: &str) -> Crew {
    let pg = backend_of(pool);
    let owner_email = format!("owner@{name}.example.com");
    let owner = an_account(pool, &owner_email).await;
    let org = backend::memberships(&pg)
        .found(name, &owner_email)
        .await
        .expect("founded");
    let ada_email = format!("ada@{name}.example.com");
    let bob_email = format!("bob@{name}.example.com");
    let ada = an_account(pool, &ada_email).await;
    let bob = an_account(pool, &bob_email).await;
    for email in [&ada_email, &bob_email] {
        backend::memberships(&pg)
            .grant(org, email, OrgRole::Member)
            .await
            .expect("granted");
    }
    let team = committed!(pg, |teams: Teams| {
        assert_eq!(
            teams
                .create(org, &TeamName::try_from("Payments").expect("a name"))
                .await
                .expect("created"),
            Named::Free
        );
        let team = teams.of(org).await.expect("listed")[0].id;
        assert!(
            teams
                .seat(org, team, &[owner, ada, bob])
                .await
                .expect("seated")
        );
        team
    });
    Crew {
        org,
        owner,
        ada,
        bob,
        team,
    }
}

fn draft(name: &str, people: &[AccountId]) -> ScheduleDraft {
    ScheduleDraft {
        name: ScheduleName::try_from(name).expect("a name"),
        timezone: Timezone::try_from("Europe/Paris").expect("a timezone"),
        rotation: Rotation::new(people.to_vec()).expect("a rotation"),
        handoff: Handoff {
            weekday: Weekday::Monday,
            time: TimeOfDay::try_from("09:30").expect("a time"),
        },
        length: RotationLength::try_from(7).expect("a length"),
        starts_on: Day::try_from("2026-11-02").expect("a day"),
    }
}

async fn a_schedule(pg: &Postgres<FixedClock>, crew: &Crew) -> Schedule {
    let rotation = [crew.owner, crew.ada, crew.bob];
    committed!(*pg, |schedules: Schedules| schedules
        .create(crew.org, crew.team, &draft("Primary", &rotation))
        .await
        .expect("kept")
        .expect("fresh"))
}

async fn a_rule(pg: &Postgres<FixedClock>, crew: &Crew, page: Option<ScheduleId>) -> RuleId {
    committed!(*pg, |rules: AlertRules| rules
        .create(
            crew.org,
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
                        placement: ferristrace_domain::Placement::default(),
                    }),
                    condition: Condition::new(Comparison::Above, 5.0).expect("a threshold"),
                    window_secs: 300,
                    pending_secs: 0,
                    severity: AlertSeverity::Critical,
                    no_data: NoDataPolicy::Ok,
                    channels: Vec::new(),
                    page,
                    mentions: ferristrace_domain::Mentions::default(),
                })
                .expect("a definition"),
            },
            crew.owner,
        )
        .await
        .expect("kept")
        .expect("fresh")
        .id)
}

fn a_page(crew: &Crew, schedule: ScheduleId, source: PageSource, target: AccountId) -> Page {
    Page {
        id: PageId::fresh(),
        org: crew.org,
        schedule,
        target,
        source,
        headline: "Checkout errors".into(),
        state: PageState::Triggered,
        step: 0,
        created_ms: NOW * 1_000,
        escalate_at_ms: Some((NOW + 900) * 1_000),
        acknowledged_by: None,
        acknowledged_ms: None,
        resolved_ms: None,
    }
}

#[tokio::test]
async fn a_schedule_its_rotation_order_and_its_overrides_read_back_whole() {
    let Some(pool) = control_plane("oncall_round_trip", 2).await else {
        return;
    };
    let pg = backend_of(&pool);
    let crew = a_crew(&pool, "acme").await;

    let made = a_schedule(&pg, &crew).await;
    let twice = committed!(pg, |schedules: Schedules| schedules
        .create(crew.org, crew.team, &draft("Primary", &[]))
        .await
        .expect("kept"));
    let mut changed = made.clone();
    changed.rotation = Rotation::new(vec![crew.bob, crew.owner]).expect("a rotation");
    changed.handoff.weekday = Weekday::Friday;
    changed.length = RotationLength::try_from(1).expect("a length");
    let cover = Override {
        id: OverrideId::fresh(),
        schedule: made.id,
        account: crew.ada,
        from_ms: 10,
        to_ms: 20,
    };
    let (read, listed, kept_overrides, removed, missing) =
        committed!(pg, |schedules: Schedules| {
            assert!(schedules.update(&changed).await.expect("updated"));
            schedules
                .add_override(crew.org, &cover)
                .await
                .expect("kept");
            let read = schedules.get(crew.org, made.id).await.expect("read");
            let listed = schedules
                .of_team(crew.org, crew.team)
                .await
                .expect("listed");
            let kept = schedules
                .overrides(crew.org, made.id)
                .await
                .expect("listed");
            let removed = schedules
                .remove_override(crew.org, made.id, cover.id)
                .await
                .expect("removed");
            let missing = schedules
                .remove_override(crew.org, made.id, cover.id)
                .await
                .expect("asked");
            (read, listed, kept, removed, missing)
        });

    assert_eq!(made.rotation.people(), &[crew.owner, crew.ada, crew.bob]);
    assert_eq!(made.timezone.name(), "Europe/Paris");
    assert_eq!(made.handoff.time.to_string(), "09:30");
    assert_eq!(made.starts_on.to_string(), "2026-11-02");
    assert_eq!(twice, None, "a name is unique within a team");
    assert_eq!(read, Some(changed.clone()));
    assert_eq!(listed, vec![changed]);
    assert_eq!(kept_overrides, vec![cover]);
    assert!(removed);
    assert!(!missing);
}

#[tokio::test]
async fn deleting_a_schedule_unlinks_the_rules_that_paged_it() {
    let Some(pool) = control_plane("oncall_delete", 2).await else {
        return;
    };
    let pg = backend_of(&pool);
    let crew = a_crew(&pool, "acme").await;
    let schedule = a_schedule(&pg, &crew).await;
    let rule = a_rule(&pg, &crew, Some(schedule.id)).await;

    let (deleted, kept) = committed!(pg, |schedules: Schedules, rules: AlertRules| {
        let deleted = schedules
            .delete(crew.org, schedule.id)
            .await
            .expect("deleted");
        (deleted, rules.get(crew.org, rule).await.expect("read"))
    });

    assert!(deleted);
    assert_eq!(kept.expect("the rule stays").definition.page(), None);
}

#[tokio::test]
async fn schedules_and_pages_never_cross_organisations() {
    let Some(pool) = control_plane("oncall_isolation", 2).await else {
        return;
    };
    let pg = backend_of(&pool);
    let mine = a_crew(&pool, "acme").await;
    let theirs = a_crew(&pool, "globex").await;
    let schedule = a_schedule(&pg, &mine).await;
    let incident = IncidentId::fresh();
    sqlx::query(
        "INSERT INTO incidents (id, org_id, title, severity, status, opened_ms) \
         VALUES ($1, $2, 'Down', 'sev2', 'investigating', 1)",
    )
    .bind(incident.as_uuid())
    .bind(mine.org.as_uuid())
    .execute(&pool)
    .await
    .expect("an incident");
    let page = a_page(
        &mine,
        schedule.id,
        PageSource::Incident(incident),
        mine.owner,
    );
    let mut foreign = page.clone();
    foreign.id = PageId::fresh();
    foreign.org = theirs.org;

    let (seen, raised_elsewhere, raised, listed, gotten, resolved) =
        committed!(pg, |schedules: Schedules, pages: Pages| {
            let seen = schedules.get(theirs.org, schedule.id).await.expect("read");
            let raised_elsewhere = pages.raise(&foreign).await.expect("asked");
            let raised = pages.raise(&page).await.expect("raised");
            let listed = pages
                .list(theirs.org, PageScope::All)
                .await
                .expect("listed");
            let gotten = pages.get(theirs.org, page.id).await.expect("read");
            let resolved = pages
                .resolve(theirs.org, Origin::Incident(incident), 5)
                .await
                .expect("asked");
            (seen, raised_elsewhere, raised, listed, gotten, resolved)
        });

    assert_eq!(seen, None);
    assert!(
        !raised_elsewhere,
        "a page names a schedule of its own organisation"
    );
    assert!(raised);
    assert!(listed.is_empty());
    assert_eq!(gotten, None);
    assert!(resolved.is_empty());
}

#[tokio::test]
async fn a_page_is_raised_once_per_open_source_kept_resolved_and_raised_again() {
    let Some(pool) = control_plane("oncall_pages", 2).await else {
        return;
    };
    let pg = backend_of(&pool);
    let crew = a_crew(&pool, "acme").await;
    let schedule = a_schedule(&pg, &crew).await;
    let rule = a_rule(&pg, &crew, Some(schedule.id)).await;
    let source = PageSource::Alert {
        rule,
        event: EventId::fresh(),
    };
    let first = a_page(&crew, schedule.id, source, crew.owner);
    let mut second = a_page(&crew, schedule.id, source, crew.ada);
    second.id = PageId::fresh();
    let mut moved = first.clone();
    moved.state = PageState::Escalated;
    moved.step = 1;
    moved.target = crew.ada;
    moved.escalate_at_ms = Some(42);

    let (raised, again, due, kept, open, resolved, after, reraised) =
        committed!(pg, |pages: Pages| {
            let raised = pages.raise(&first).await.expect("raised");
            let again = pages.raise(&second).await.expect("asked");
            let due = pages.due((NOW + 900) * 1_000, 10).await.expect("due");
            let kept = pages.keep(&moved).await.expect("kept");
            let open = pages.list(crew.org, PageScope::Open).await.expect("listed");
            let resolved = pages
                .resolve(crew.org, Origin::Rule(rule), 77)
                .await
                .expect("resolved");
            let after = pages.list(crew.org, PageScope::Open).await.expect("listed");
            let reraised = pages.raise(&second).await.expect("raised");
            (raised, again, due, kept, open, resolved, after, reraised)
        });

    assert!(raised);
    assert!(!again, "one open page per rule");
    assert_eq!(due, vec![first.clone()]);
    assert!(kept);
    assert_eq!(open, vec![moved.clone()]);
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].state, PageState::Resolved);
    assert_eq!(resolved[0].resolved_ms, Some(77));
    assert_eq!(resolved[0].escalate_at_ms, None);
    assert!(after.is_empty());
    assert!(reraised, "a resolved page leaves room for the next one");
}

#[tokio::test]
async fn a_page_notice_is_claimed_once_with_its_address_and_settles() {
    let Some(pool) = control_plane("oncall_notices", 2).await else {
        return;
    };
    let pg = backend_of(&pool);
    let crew = a_crew(&pool, "acme").await;
    let schedule = a_schedule(&pg, &crew).await;
    let rule = a_rule(&pg, &crew, Some(schedule.id)).await;
    let page = a_page(
        &crew,
        schedule.id,
        PageSource::Alert {
            rule,
            event: EventId::fresh(),
        },
        crew.ada,
    );
    let notice = Notice {
        id: NoticeId::fresh(),
        org: crew.org,
        page: page.id,
        step: 0,
        to: crew.ada,
        attempt: 0,
        next_at_ms: NOW * 1_000,
        status: DeliveryStatus::Pending,
        last_error: None,
    };

    let (claimed, after_retry, retried) = committed!(pg, |pages: Pages| {
        pages.raise(&page).await.expect("raised");
        pages.enqueue(&notice).await.expect("queued");
        pages
            .enqueue(&Notice {
                id: NoticeId::fresh(),
                ..notice.clone()
            })
            .await
            .expect("queued once");
        let claimed: Vec<ClaimedNotice> = pages.claim(NOW * 1_000, 10).await.expect("claimed");
        pages
            .settle(
                notice.id,
                1,
                &Settlement::Retry {
                    next_at_ms: (NOW + 60) * 1_000,
                    error: "refused".into(),
                },
            )
            .await
            .expect("settled");
        let early = pages.claim(NOW * 1_000, 10).await.expect("claimed");
        let retried = pages.claim((NOW + 60) * 1_000, 10).await.expect("claimed");
        pages
            .settle(notice.id, 2, &Settlement::Sent)
            .await
            .expect("settled");
        (claimed, early, retried)
    });

    assert_eq!(claimed.len(), 1, "a step mails a person once");
    assert_eq!(claimed[0].email.as_str(), "ada@acme.example.com");
    assert_eq!(claimed[0].dispatch.headline.as_ref(), "Checkout errors");
    assert_eq!(claimed[0].dispatch.source, page.source);
    assert!(after_retry.is_empty());
    assert_eq!(retried[0].notice.attempt, 1);
    let status: String = sqlx::query_scalar("SELECT status FROM page_outbox")
        .fetch_one(&pool)
        .await
        .expect("a row");
    assert_eq!(status, "sent");
}

async fn escalated(pg: &Postgres<FixedClock>, mail: &Couriered, at: u64) -> Paged {
    UnitOfWork::transaction(
        pg,
        || OncallError::StoreUnreachable,
        Settle::OnSuccess,
        |tx| async move {
            Pager::new(
                Paging::new(
                    <Postgres<FixedClock> as RepoFor<domain::Schedules>>::build(pg, &tx),
                    <Postgres<FixedClock> as RepoFor<domain::Pages>>::build(pg, &tx),
                ),
                <Postgres<FixedClock> as RepoFor<domain::AlertDuty>>::build(pg, &tx),
                <Postgres<FixedClock> as RepoFor<domain::Roster>>::build(pg, &tx),
                <Postgres<FixedClock> as RepoFor<domain::IncidentTimeline>>::build(pg, &tx),
                <Postgres<FixedClock> as RepoFor<domain::Audit>>::build(pg, &tx),
                mail,
                None,
            )
            .tick(UnixSeconds::new(at))
            .await
        },
    )
    .await
    .expect("a tick")
}

#[tokio::test]
async fn two_concurrent_escalation_ticks_escalate_once() {
    let Some(pool) = control_plane("oncall_replicas", 4).await else {
        return;
    };
    let pg = backend_of(&pool);
    let crew = a_crew(&pool, "acme").await;
    let schedule = a_schedule(&pg, &crew).await;
    let rule = a_rule(&pg, &crew, Some(schedule.id)).await;
    let page = a_page(
        &crew,
        schedule.id,
        PageSource::Alert {
            rule,
            event: EventId::fresh(),
        },
        crew.owner,
    );
    committed!(pg, |pages: Pages| pages.raise(&page).await.expect("raised"));
    let mail = Couriered::default();

    let (one, other) = tokio::join!(
        escalated(&pg, &mail, NOW + 900),
        escalated(&pg, &mail, NOW + 900)
    );
    let same_slot = escalated(&pg, &mail, NOW + 910).await;

    let mut outcomes = [one, other];
    outcomes.sort_by_key(|outcome| matches!(outcome, Paged::Skipped));
    assert_eq!(
        outcomes[0],
        Paged::Worked {
            escalated: 1,
            sent: 1,
            retried: 0,
            abandoned: 0,
        }
    );
    assert_eq!(outcomes[1], Paged::Skipped);
    assert_eq!(same_slot, Paged::Skipped);
    let (step, target): (i32, Uuid) = sqlx::query_as("SELECT step, target_id FROM pages")
        .fetch_one(&pool)
        .await
        .expect("a page");
    assert_eq!((step, target), (1, crew.ada.as_uuid()));
    assert_eq!(mail.paged().len(), 1);
    assert_eq!(mail.paged()[0].0.as_ref(), "ada@acme.example.com");
    let logged = committed!(pg, |audit: Audit| audit
        .list(crew.org, &AuditFilter::default(), PageSize::default())
        .await
        .expect("listed"));
    let escalation = logged
        .entries
        .iter()
        .find(|logged| logged.entry.action == Action::PageEscalated)
        .expect("an escalation entry");
    assert_eq!(escalation.entry.actor, Actor::System);
    assert_eq!(escalation.entry.target.kind, TargetKind::Page);
}

#[tokio::test]
async fn the_system_writes_audit_entries_without_an_id() {
    let Some(pool) = control_plane("oncall_system_actor", 2).await else {
        return;
    };
    let pg = backend_of(&pool);
    let crew = a_crew(&pool, "acme").await;

    let logged = committed!(pg, |audit: Audit| {
        audit
            .record(&AuditEntry {
                org: crew.org,
                actor: Actor::System,
                action: Action::PageEscalated,
                target: Target::of(TargetKind::Page, Some("p".into())),
                detail: Detail::default().with("step", 1),
            })
            .await
            .expect("kept");
        audit
            .list(
                crew.org,
                &AuditFilter {
                    actor: Some(Actor::System),
                    ..AuditFilter::default()
                },
                PageSize::default(),
            )
            .await
            .expect("listed")
    });

    assert_eq!(logged.entries.len(), 1);
    assert_eq!(logged.entries[0].entry.actor, Actor::System);
}

#[tokio::test]
async fn an_incident_keeps_its_paged_schedule_and_its_paging_entries() {
    let Some(pool) = control_plane("oncall_incidents", 2).await else {
        return;
    };
    let pg = backend_of(&pool);
    let crew = a_crew(&pool, "acme").await;
    let schedule = a_schedule(&pg, &crew).await;
    let incident = Incident {
        id: IncidentId::fresh(),
        org: crew.org,
        project: None,
        title: IncidentTitle::try_from("Checkout down").expect("a title"),
        severity: IncidentSeverity::Sev2,
        status: IncidentStatus::Investigating,
        owner: None,
        public: false,
        opened_ms: 1,
        resolved_ms: None,
        opened_from_alert: None,
        paged_schedule: Some(schedule.id),
    };
    let page = PageId::fresh();
    let entries = [
        (
            Author::System,
            Change::Paged {
                page,
                step: 0,
                target: Some(crew.owner),
            },
        ),
        (
            Author::System,
            Change::Paged {
                page,
                step: 3,
                target: None,
            },
        ),
        (Author::Account(crew.ada), Change::Acknowledged { page }),
    ];

    let (read, timeline) = committed!(pg, |incidents: Incidents, timeline: IncidentTimeline| {
        incidents.open(&incident).await.expect("opened");
        for (at, (author, change)) in entries.iter().enumerate() {
            timeline
                .append(&TimelineEntry {
                    id: TimelineId::fresh(),
                    incident: incident.id,
                    org: crew.org,
                    author: author.clone(),
                    change: change.clone(),
                    at_ms: u64::try_from(at).expect("small"),
                })
                .await
                .expect("appended");
        }
        (
            incidents.get(crew.org, incident.id).await.expect("read"),
            timeline.of(crew.org, incident.id).await.expect("read"),
        )
    });

    assert_eq!(read, Some(incident));
    assert_eq!(
        timeline
            .into_iter()
            .map(|entry| (entry.author, entry.change))
            .collect::<Vec<_>>(),
        entries.to_vec()
    );
}
