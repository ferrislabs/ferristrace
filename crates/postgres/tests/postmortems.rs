use ferristrace_domain::{
    AccountId, Incident, IncidentId, IncidentSeverity, IncidentStatus, IncidentTitle, Incidents,
    Postmortem, PostmortemBody, PostmortemId, PostmortemStatus, Postmortems, TenantId,
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

fn an_incident(org: TenantId) -> Incident {
    Incident {
        id: IncidentId::fresh(),
        org,
        project: None,
        title: IncidentTitle::try_from("Checkout down").expect("a title"),
        severity: IncidentSeverity::Sev2,
        status: IncidentStatus::Investigating,
        owner: None,
        public: false,
        opened_ms: 1_000,
        resolved_ms: None,
        opened_from_alert: None,
        paged_schedule: None,
    }
}

fn a_postmortem(incident: &Incident, author: AccountId, text: &str) -> Postmortem {
    Postmortem {
        id: PostmortemId::fresh(),
        org: incident.org,
        incident: incident.id,
        status: PostmortemStatus::Draft,
        body: PostmortemBody::try_from(text).expect("a body"),
        author: Some(author),
        edited: false,
        created_ms: 2_000,
        updated_ms: 2_000,
        published_ms: None,
    }
}

#[tokio::test]
async fn a_postmortem_reads_back_whole_after_every_change() {
    let Some(pool) = control_plane("postmortems_round_trip").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, author) = an_org(&pool, "Acme", "owner@example.com").await;
    let incident = an_incident(org);
    let drafted = a_postmortem(&incident, author, "# Postmortem\n\n- [ ] \n");
    let mut published = drafted.clone();
    published.status = PostmortemStatus::Published;
    published.body = PostmortemBody::try_from("edited words").expect("a body");
    published.edited = true;
    published.updated_ms = 3_000;
    published.published_ms = Some(3_000);

    let (none_yet, created, first, kept, second, listed) =
        committed!(pg, |incidents: Incidents, postmortems: Postmortems| {
            incidents.open(&incident).await.expect("opened");
            let none_yet = postmortems.of(org, incident.id).await.expect("a read");
            let created = postmortems.create(&drafted).await.expect("created");
            let first = postmortems.of(org, incident.id).await.expect("a read");
            let kept = postmortems.keep(&published).await.expect("kept");
            let second = postmortems.of(org, incident.id).await.expect("a read");
            let listed = postmortems.summaries(org).await.expect("a read");
            (none_yet, created, first, kept, second, listed)
        });

    assert_eq!(none_yet, None);
    assert!(created);
    assert_eq!(first, Some(drafted));
    assert!(kept);
    assert_eq!(second, Some(published));
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].incident, incident.id);
    assert_eq!(listed[0].status, PostmortemStatus::Published);
}

#[tokio::test]
async fn an_incident_holds_at_most_one_postmortem() {
    let Some(pool) = control_plane("postmortems_unique").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, author) = an_org(&pool, "Acme", "owner@example.com").await;
    let incident = an_incident(org);
    let first = a_postmortem(&incident, author, "first");
    let rival = a_postmortem(&incident, author, "rival");

    let (created, again, held) =
        committed!(pg, |incidents: Incidents, postmortems: Postmortems| {
            incidents.open(&incident).await.expect("opened");
            let created = postmortems.create(&first).await.expect("created");
            let again = postmortems.create(&rival).await.expect("refused quietly");
            let held = postmortems.of(org, incident.id).await.expect("a read");
            (created, again, held)
        });

    assert!(created);
    assert!(!again);
    assert_eq!(held, Some(first));
}

#[tokio::test]
async fn one_organisation_never_sees_or_writes_another_organisations_postmortems() {
    let Some(pool) = control_plane("postmortems_isolation").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, author) = an_org(&pool, "Acme", "owner@example.com").await;
    let (theirs, stranger) = an_org(&pool, "Other", "other@example.com").await;
    let mine = an_incident(org);
    let kept = a_postmortem(&mine, author, "mine");
    let mut forged = a_postmortem(&mine, stranger, "forged");
    forged.org = theirs;
    let mut taken = kept.clone();
    taken.org = theirs;
    taken.body = PostmortemBody::try_from("taken over").expect("a body");
    let orphan = a_postmortem(&an_incident(org), author, "no incident");

    let (forged_done, orphan_done, seen, listed, moved, held) =
        committed!(pg, |incidents: Incidents, postmortems: Postmortems| {
            incidents.open(&mine).await.expect("opened");
            postmortems.create(&kept).await.expect("created");
            let forged_done = postmortems.create(&forged).await.expect("refused quietly");
            let orphan_done = postmortems.create(&orphan).await.expect("refused quietly");
            let seen = postmortems.of(theirs, mine.id).await.expect("a read");
            let listed = postmortems.summaries(theirs).await.expect("a read");
            let moved = postmortems.keep(&taken).await.expect("a write");
            let held = postmortems.of(org, mine.id).await.expect("a read");
            (forged_done, orphan_done, seen, listed, moved, held)
        });

    assert!(!forged_done);
    assert!(!orphan_done);
    assert_eq!(seen, None);
    assert!(listed.is_empty());
    assert!(!moved);
    assert_eq!(held, Some(kept));
}
