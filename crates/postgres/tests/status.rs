use std::collections::BTreeSet;
use std::time::Duration;

use ferristrace_application::registry::{RepoFor, Settle, UnitOfWork, domain};
use ferristrace_domain::{
    Accent, AccountId, Announcement, ClaimedLetter, Component, ComponentId, ComponentName,
    ComponentStatus, Email, Incident, IncidentId, IncidentSeverity, IncidentStatus, IncidentTitle,
    Incidents, Intro, Invite, LetterBody, LetterId, Link, Mode, PageName, ProjectId, Saved,
    ServiceName, Settlement, Slug, StatusOutbox, StatusPage, StatusPageId, StatusPages,
    StatusSubscribers, Subscriber, SubscriberId, TenantId, Theme, UpdateKind,
};
use ferristrace_infra::FixedClock;
use ferristrace_postgres::{CONTROL_PLANE, Postgres};
use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool};
use uuid::Uuid;

#[macro_use]
mod backend;

const NOW: u64 = 1_700_000_040;

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

async fn a_project(pool: &PgPool, org: TenantId) -> ProjectId {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO projects (id, org_id, name) VALUES ($1, $2, 'storefront')")
        .bind(id)
        .bind(org.as_uuid())
        .execute(pool)
        .await
        .expect("inserting a project");
    ProjectId::new(id)
}

fn a_page(org: TenantId, slug: &str, public: bool) -> StatusPage {
    StatusPage {
        id: StatusPageId::fresh(),
        org,
        name: PageName::try_from("Acme").expect("a name"),
        slug: Slug::try_from(slug).expect("a slug"),
        public,
        hide_from_search: true,
        theme: Theme {
            accent: Accent::Rose,
            mode: Mode::Dark,
        },
        intro: Some(Intro::try_from("Live status of Acme").expect("an intro")),
        created_ms: 1_000,
        updated_ms: 2_000,
    }
}

fn a_component(page: &StatusPage, name: &str, link: Link, position: u32) -> Component {
    Component {
        id: ComponentId::fresh(),
        page: page.id,
        name: ComponentName::try_from(name).expect("a name"),
        link,
        manual_status: None,
        position,
        public: true,
    }
}

fn a_subscriber(page: &StatusPage, email: &str, confirmed: bool) -> Subscriber {
    Subscriber {
        id: SubscriberId::fresh(),
        page: page.id,
        email: Email::try_from(email).expect("an address"),
        confirmed,
        invited_ms: 3_000,
    }
}

fn an_incident(org: TenantId) -> Incident {
    Incident {
        id: IncidentId::fresh(),
        org,
        project: None,
        title: IncidentTitle::try_from("Payments fail").expect("a title"),
        severity: IncidentSeverity::Sev2,
        status: IncidentStatus::Identified,
        owner: None,
        public: true,
        opened_ms: 1_000,
        resolved_ms: None,
        opened_from_alert: None,
        paged_schedule: None,
    }
}

fn an_announcement(incident: &Incident, text: &str) -> Announcement {
    Announcement {
        org: incident.org,
        incident: incident.id,
        title: incident.title.clone(),
        kind: UpdateKind::Comment,
        status: incident.status,
        text: text.into(),
        at_ms: NOW * 1_000,
    }
}

#[tokio::test]
async fn a_page_and_its_components_read_back_whole() {
    let Some(pool) = control_plane("status_round_trip", 2).await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, _) = an_org(&pool, "Acme", "owner@example.com").await;
    let project = a_project(&pool, org).await;
    let page = a_page(org, "acme", true);
    let service = a_component(
        &page,
        "Checkout",
        Link::Service {
            name: ServiceName::try_from("checkout-api").expect("a service"),
            project: Some(project),
        },
        0,
    );
    let mut manual = a_component(&page, "Payouts", Link::Manual, 1);
    manual.manual_status = Some(ComponentStatus::Degraded);
    manual.public = false;
    let mut renamed = page.clone();
    renamed.name = PageName::try_from("Acme Inc").expect("a name");
    renamed.slug = Slug::try_from("acme-inc").expect("a slug");
    renamed.intro = None;
    renamed.public = false;

    let (created, by_slug, components, kept, again) = committed!(pg, |pages: StatusPages| {
        let created = pages.create(&page).await.expect("created");
        let by_slug = pages.by_slug(&page.slug).await.expect("read");
        pages.add_component(org, &manual).await.expect("added");
        pages.add_component(org, &service).await.expect("added");
        let components = pages.components(org, page.id).await.expect("read");
        let kept = pages.keep(&renamed).await.expect("kept");
        let again = pages.get(org, page.id).await.expect("read");
        (created, by_slug, components, kept, again)
    });

    assert_eq!(created, Saved::Done);
    assert_eq!(by_slug, Some(page.clone()));
    assert_eq!(components, vec![service, manual]);
    assert_eq!(kept, Saved::Done);
    assert_eq!(again, Some(renamed));
}

#[tokio::test]
async fn a_slug_is_unique_across_organisations() {
    let Some(pool) = control_plane("status_slugs", 2).await else {
        return;
    };
    let pg = backend_of(&pool);
    let (mine, _) = an_org(&pool, "Acme", "owner@example.com").await;
    let (theirs, _) = an_org(&pool, "Globex", "owner@globex.example.com").await;
    let first = a_page(mine, "taken", true);
    let second = a_page(theirs, "taken", true);
    let mut renamed = a_page(theirs, "free", true);

    let (one, two, three, four, listed) = committed!(pg, |pages: StatusPages| {
        let one = pages.create(&first).await.expect("created");
        let two = pages.create(&second).await.expect("refused");
        let three = pages.create(&renamed).await.expect("created");
        renamed.slug = Slug::try_from("taken").expect("a slug");
        let four = pages.keep(&renamed).await.expect("refused");
        let listed = pages.list(theirs).await.expect("listed");
        (one, two, three, four, listed)
    });

    assert_eq!(
        (one, two, three, four),
        (Saved::Done, Saved::SlugTaken, Saved::Done, Saved::SlugTaken)
    );
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].slug.as_str(), "free");
}

#[tokio::test]
async fn components_are_kept_per_organisation_and_go_with_their_page() {
    let Some(pool) = control_plane("status_components", 2).await else {
        return;
    };
    let pg = backend_of(&pool);
    let (mine, _) = an_org(&pool, "Acme", "owner@example.com").await;
    let (theirs, _) = an_org(&pool, "Globex", "owner@globex.example.com").await;
    let page = a_page(mine, "acme", true);
    let mut one = a_component(&page, "One", Link::Manual, 0);

    let (foreign, moved, removed_by_them, deleted, left) = committed!(pg, |pages: StatusPages| {
        pages.create(&page).await.expect("created");
        let foreign = pages.add_component(theirs, &one).await.expect("refused");
        pages.add_component(mine, &one).await.expect("added");
        one.position = 5;
        let moved = pages.keep_component(mine, &one).await.expect("moved");
        let removed_by_them = pages
            .remove_component(theirs, page.id, one.id)
            .await
            .expect("refused");
        let deleted = pages.delete(mine, page.id).await.expect("deleted");
        let left = pages.components(mine, page.id).await.expect("read");
        (foreign, moved, removed_by_them, deleted, left)
    });

    assert!(!foreign);
    assert!(moved);
    assert!(!removed_by_them);
    assert!(deleted);
    assert!(left.is_empty());
}

#[tokio::test]
async fn a_subscriber_is_kept_once_per_page_confirmed_and_forgotten() {
    let Some(pool) = control_plane("status_subscribers", 2).await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, _) = an_org(&pool, "Acme", "owner@example.com").await;
    let page = a_page(org, "acme", true);
    let ada = a_subscriber(&page, "ada@example.com", false);
    let twin = a_subscriber(&page, "ada@example.com", false);

    let (first, second, found, confirmed, counted, removed, gone) =
        committed!(pg, |pages: StatusPages, subscribers: StatusSubscribers| {
            pages.create(&page).await.expect("created");
            let first = subscribers.add(&ada).await.expect("added");
            let second = subscribers.add(&twin).await.expect("refused");
            let found = subscribers.find(page.id, &ada.email).await.expect("found");
            let confirmed = subscribers
                .keep(&Subscriber {
                    confirmed: true,
                    invited_ms: 9_000,
                    ..ada.clone()
                })
                .await
                .expect("kept");
            let counted = subscribers.confirmed(page.id).await.expect("counted");
            let removed = subscribers.remove(ada.id).await.expect("removed");
            let gone = subscribers.get(ada.id).await.expect("read");
            (first, second, found, confirmed, counted, removed, gone)
        });

    assert!(first);
    assert!(!second, "one subscription per page and address");
    assert_eq!(found, Some(ada));
    assert!(confirmed);
    assert_eq!(counted, 1);
    assert!(removed);
    assert_eq!(gone, None);
}

#[tokio::test]
async fn an_announcement_reaches_confirmed_subscribers_of_public_pages_of_its_organisation() {
    let Some(pool) = control_plane("status_announce", 2).await else {
        return;
    };
    let pg = backend_of(&pool);
    let (mine, _) = an_org(&pool, "Acme", "owner@example.com").await;
    let (theirs, _) = an_org(&pool, "Globex", "owner@globex.example.com").await;
    let public = a_page(mine, "acme", true);
    let private = a_page(mine, "acme-internal", false);
    let foreign = a_page(theirs, "globex", true);
    let incident = an_incident(mine);
    let pending = a_subscriber(&public, "pending@example.com", false);

    let (reached, claimed) =
        committed!(pg, |pages: StatusPages,
                        subscribers: StatusSubscribers,
                        outbox: StatusOutbox,
                        incidents: Incidents| {
            for page in [&public, &private, &foreign] {
                pages.create(page).await.expect("created");
            }
            for subscriber in [
                a_subscriber(&public, "one@example.com", true),
                a_subscriber(&public, "two@example.com", true),
                pending.clone(),
                a_subscriber(&private, "hidden@example.com", true),
                a_subscriber(&foreign, "stranger@example.com", true),
            ] {
                subscribers.add(&subscriber).await.expect("added");
            }
            incidents.open(&incident).await.expect("opened");
            outbox
                .invite(&Invite {
                    id: LetterId::fresh(),
                    page: public.id,
                    subscriber: pending.id,
                    at_ms: NOW * 1_000,
                })
                .await
                .expect("invited");
            let reached = outbox
                .announce(&an_announcement(&incident, "Some payments fail."))
                .await
                .expect("announced");
            let claimed: Vec<ClaimedLetter> = outbox.claim(NOW * 1_000, 50).await.expect("claimed");
            (reached, claimed)
        });

    assert_eq!(reached, 2);
    let mut addressed: Vec<(&str, bool)> = claimed
        .iter()
        .map(|letter| {
            (
                letter.email.as_str(),
                matches!(letter.body, LetterBody::Confirm),
            )
        })
        .collect();
    addressed.sort_unstable();
    assert_eq!(
        addressed,
        vec![
            ("one@example.com", false),
            ("pending@example.com", true),
            ("two@example.com", false)
        ]
    );
    let update = claimed
        .iter()
        .find_map(|letter| match &letter.body {
            LetterBody::Update(bulletin) => Some(bulletin.clone()),
            LetterBody::Confirm => None,
        })
        .expect("an update");
    assert_eq!(&*update.text, "Some payments fail.");
    assert_eq!(update.title.as_str(), "Payments fail");
    assert_eq!(update.kind, UpdateKind::Comment);
    assert_eq!(update.status, IncidentStatus::Identified);
    assert!(claimed.iter().all(|letter| letter.slug.as_str() == "acme"));
}

async fn claim_three(pg: &Postgres<FixedClock>) -> Vec<LetterId> {
    UnitOfWork::transaction(
        pg,
        || (),
        Settle::OnSuccess,
        |tx| async move {
            let outbox = <Postgres<FixedClock> as RepoFor<domain::StatusOutbox>>::build(pg, &tx);
            let claimed = outbox.claim(NOW * 1_000, 3).await.expect("claimed");
            tokio::time::sleep(Duration::from_millis(300)).await;
            for held in &claimed {
                outbox
                    .settle(held.id, 1, &Settlement::Sent)
                    .await
                    .expect("settled");
            }
            Ok::<_, ()>(claimed.iter().map(|held| held.id).collect::<Vec<_>>())
        },
    )
    .await
    .expect("committed")
}

#[tokio::test]
async fn a_letter_is_claimed_once_under_two_drains_and_settles() {
    let Some(pool) = control_plane("status_claims", 4).await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, _) = an_org(&pool, "Acme", "owner@example.com").await;
    let page = a_page(org, "acme", true);
    let incident = an_incident(org);
    committed!(pg, |pages: StatusPages,
                    subscribers: StatusSubscribers,
                    outbox: StatusOutbox,
                    incidents: Incidents| {
        pages.create(&page).await.expect("created");
        for seed in 0..5 {
            subscribers
                .add(&a_subscriber(&page, &format!("s{seed}@example.com"), true))
                .await
                .expect("added");
        }
        incidents.open(&incident).await.expect("opened");
        outbox
            .announce(&an_announcement(&incident, "Still failing."))
            .await
            .expect("announced")
    });

    let (one, other) = tokio::join!(claim_three(&pg), claim_three(&pg));

    let one: BTreeSet<_> = one.into_iter().collect();
    let other: BTreeSet<_> = other.into_iter().collect();
    assert!(one.is_disjoint(&other), "no letter is claimed twice");
    assert_eq!(one.len() + other.len(), 5);
    let left = committed!(pg, |outbox: StatusOutbox| outbox
        .claim(NOW * 1_000 + 60_000, 50)
        .await
        .expect("claimed"));
    assert!(left.is_empty());
}

#[tokio::test]
async fn a_retried_letter_waits_and_an_abandoned_one_is_never_claimed_again() {
    let Some(pool) = control_plane("status_retries", 2).await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, _) = an_org(&pool, "Acme", "owner@example.com").await;
    let page = a_page(org, "acme", true);
    let incident = an_incident(org);

    let (early, due, abandoned) =
        committed!(pg, |pages: StatusPages,
                        subscribers: StatusSubscribers,
                        outbox: StatusOutbox,
                        incidents: Incidents| {
            pages.create(&page).await.expect("created");
            subscribers
                .add(&a_subscriber(&page, "ada@example.com", true))
                .await
                .expect("added");
            incidents.open(&incident).await.expect("opened");
            outbox
                .announce(&an_announcement(&incident, "Still failing."))
                .await
                .expect("announced");
            let first = outbox.claim(NOW * 1_000, 10).await.expect("claimed");
            outbox
                .settle(
                    first[0].id,
                    1,
                    &Settlement::Retry {
                        next_at_ms: NOW * 1_000 + 60_000,
                        error: "451".into(),
                    },
                )
                .await
                .expect("settled");
            let early = outbox.claim(NOW * 1_000, 10).await.expect("claimed");
            let due = outbox
                .claim(NOW * 1_000 + 60_000, 10)
                .await
                .expect("claimed");
            outbox
                .settle(
                    first[0].id,
                    2,
                    &Settlement::Abandoned {
                        error: "451".into(),
                    },
                )
                .await
                .expect("settled");
            let abandoned = outbox
                .claim(NOW * 1_000 + 999_000, 10)
                .await
                .expect("claimed");
            (early.len(), due, abandoned.len())
        });

    assert_eq!(early, 0);
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].attempt, 1);
    assert_eq!(abandoned, 0);
}
