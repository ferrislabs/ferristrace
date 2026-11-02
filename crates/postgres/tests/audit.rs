use std::path::{Path, PathBuf};

use ferristrace_application::registry::{RepoFor, Settle, UnitOfWork, domain};
use ferristrace_domain::{
    AccountId, Action, Actor, ApiKeys, AuditEntry, AuditFilter, AuditTrail, Detail, KeyName,
    KeyScopes, NoPhotos, OrgName, OrgRole, OrgSettings, Organisation, Orgs, PageSize, SessionStore,
    SessionToken, SettingError, Target, TenantId, Whoami,
};
use ferristrace_infra::FixedClock;
use ferristrace_postgres::{CONTROL_PLANE, Postgres};
use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool};
use uuid::Uuid;

#[macro_use]
mod backend;

const NOW: u64 = 1_700_000_000;

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

async fn an_org(pool: &PgPool, name: &str, owner: &str) -> (TenantId, AccountId) {
    let account = an_account(pool, owner).await;
    let org = backend::memberships(&backend_of(pool))
        .found(name, owner)
        .await
        .expect("the organisation is founded");
    (org, account)
}

fn renamed(org: TenantId, by: Actor, to: &str) -> AuditEntry {
    AuditEntry {
        org,
        actor: by,
        action: Action::OrgRenamed,
        target: Target::org(org),
        detail: Detail::default().with("to", to),
    }
}

fn founded(org: TenantId) -> AuditEntry {
    AuditEntry {
        org,
        actor: Actor::Operator,
        action: Action::OrgFounded,
        target: Target::org(org),
        detail: Detail::default(),
    }
}

fn names(page: &ferristrace_domain::AuditPage) -> Vec<Option<String>> {
    page.entries
        .iter()
        .map(|logged| {
            logged
                .entry
                .detail
                .get("to")
                .flatten()
                .map(ToOwned::to_owned)
        })
        .collect()
}

#[tokio::test]
async fn a_recorded_entry_reads_back_whole_with_the_account_as_its_label() {
    let Some(pool) = control_plane("audit_round_trip").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "Acme", "owner@example.com").await;
    let trail = backend::audit(&pg);
    let entry = AuditEntry {
        org,
        actor: Actor::Account(owner),
        action: Action::ProjectAssigned,
        target: Target::of(ferristrace_domain::TargetKind::Project, Some("p-1".into())),
        detail: Detail::default()
            .with("project", "p-1")
            .with_maybe("team", None::<&str>),
    };

    trail.record(&entry).await.expect("a recorded entry");
    let page = trail
        .list(org, &AuditFilter::default(), PageSize::default())
        .await
        .expect("the log");

    assert_eq!(page.entries.len(), 1);
    let logged = &page.entries[0];
    assert_eq!(logged.entry.org, entry.org);
    assert_eq!(logged.entry.actor, entry.actor);
    assert_eq!(logged.entry.action, entry.action);
    assert_eq!(logged.entry.target, entry.target);
    assert_eq!(logged.entry.detail.get("project"), Some(Some("p-1")));
    assert_eq!(logged.entry.detail.get("team"), Some(None));
    assert_eq!(logged.label.as_deref(), Some("owner@example.com"));
    assert!(logged.at_unix_nanos >= NOW * 1_000_000_000);
    assert_eq!(page.next, None);
}

#[tokio::test]
async fn the_log_reads_newest_first_even_within_one_second() {
    let Some(pool) = control_plane("audit_newest_first").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "Acme", "owner@example.com").await;
    let trail = backend::audit(&pg);

    for to in ["One", "Two", "Three"] {
        trail
            .record(&renamed(org, Actor::Account(owner), to))
            .await
            .expect("a recorded entry");
    }
    let page = trail
        .list(org, &AuditFilter::default(), PageSize::default())
        .await
        .expect("the log");

    assert_eq!(
        names(&page),
        vec![
            Some("Three".to_owned()),
            Some("Two".to_owned()),
            Some("One".to_owned())
        ]
    );
}

#[tokio::test]
async fn the_log_filters_by_action_and_by_actor() {
    let Some(pool) = control_plane("audit_filters").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "Acme", "owner@example.com").await;
    let other = an_account(&pool, "other@example.com").await;
    let trail = backend::audit(&pg);
    trail.record(&founded(org)).await.expect("an entry");
    trail
        .record(&renamed(org, Actor::Account(owner), "By owner"))
        .await
        .expect("an entry");
    trail
        .record(&renamed(org, Actor::Account(other), "By other"))
        .await
        .expect("an entry");

    let by_action = trail
        .list(
            org,
            &AuditFilter {
                action: Some(Action::OrgFounded),
                ..AuditFilter::default()
            },
            PageSize::default(),
        )
        .await
        .expect("the founding");
    let by_owner = trail
        .list(
            org,
            &AuditFilter {
                actor: Some(Actor::Account(owner)),
                ..AuditFilter::default()
            },
            PageSize::default(),
        )
        .await
        .expect("the owner's");
    let by_operator = trail
        .list(
            org,
            &AuditFilter {
                actor: Some(Actor::Operator),
                ..AuditFilter::default()
            },
            PageSize::default(),
        )
        .await
        .expect("the operator's");

    assert_eq!(by_action.entries.len(), 1);
    assert_eq!(by_action.entries[0].entry.action, Action::OrgFounded);
    assert_eq!(by_action.entries[0].label, None);
    assert_eq!(names(&by_owner), vec![Some("By owner".to_owned())]);
    assert_eq!(by_operator.entries.len(), 1);
    assert_eq!(by_operator.entries[0].entry.actor, Actor::Operator);
}

#[tokio::test]
async fn a_cursor_reads_the_page_after_the_last_entry_it_names() {
    let Some(pool) = control_plane("audit_cursor").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "Acme", "owner@example.com").await;
    let trail = backend::audit(&pg);
    for to in ["One", "Two", "Three", "Four", "Five"] {
        trail
            .record(&renamed(org, Actor::Account(owner), to))
            .await
            .expect("a recorded entry");
    }

    let mut seen = Vec::new();
    let mut before = None;
    let mut pages = 0;
    loop {
        let page = trail
            .list(
                org,
                &AuditFilter {
                    before,
                    ..AuditFilter::default()
                },
                PageSize::new(Some(2)),
            )
            .await
            .expect("a page");
        pages += 1;
        seen.extend(names(&page));
        match page.next {
            Some(next) => before = Some(next),
            None => break,
        }
    }

    assert_eq!(pages, 3);
    assert_eq!(
        seen,
        ["Five", "Four", "Three", "Two", "One"]
            .map(|name| Some(name.to_owned()))
            .to_vec()
    );
}

#[tokio::test]
async fn one_organisation_never_lists_another_organisations_entries() {
    let Some(pool) = control_plane("audit_isolation").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (mine, owner) = an_org(&pool, "Acme", "owner@example.com").await;
    let (theirs, stranger) = an_org(&pool, "Globex", "stranger@example.com").await;
    let trail = backend::audit(&pg);
    trail
        .record(&renamed(mine, Actor::Account(owner), "Mine"))
        .await
        .expect("an entry");
    trail
        .record(&renamed(theirs, Actor::Account(stranger), "Theirs"))
        .await
        .expect("an entry");

    let page = trail
        .list(mine, &AuditFilter::default(), PageSize::default())
        .await
        .expect("my log");
    let filtered = trail
        .list(
            mine,
            &AuditFilter {
                actor: Some(Actor::Account(stranger)),
                ..AuditFilter::default()
            },
            PageSize::default(),
        )
        .await
        .expect("my log, filtered on a stranger");

    assert_eq!(names(&page), vec![Some("Mine".to_owned())]);
    assert!(filtered.entries.is_empty());
}

#[tokio::test]
async fn a_key_actor_is_labelled_with_the_key_name() {
    let Some(pool) = control_plane("audit_key_label").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "Acme", "owner@example.com").await;
    let issued = backend::api_keys(&pg)
        .issue(
            org,
            None,
            &KeyName::try_from("collector").expect("a name"),
            &KeyScopes::ingest(),
            owner,
        )
        .await
        .expect("a key");
    let trail = backend::audit(&pg);
    trail
        .record(&renamed(org, Actor::ApiKey(issued.key.id), "By key"))
        .await
        .expect("an entry");

    let page = trail
        .list(org, &AuditFilter::default(), PageSize::default())
        .await
        .expect("the log");

    assert_eq!(page.entries[0].label.as_deref(), Some("collector"));
}

async fn rename_through_the_repositories(
    pg: &Postgres<FixedClock>,
    token: &SessionToken,
    org: TenantId,
    name: &str,
) -> Result<Organisation, SettingError> {
    let name = OrgName::try_from(name).expect("a name");
    UnitOfWork::transaction(
        pg,
        || SettingError::StoreUnreachable,
        Settle::OnSuccess,
        |tx| {
            let sessions = <_ as RepoFor<domain::Sessions>>::build(pg, &tx);
            let memberships = <_ as RepoFor<domain::Memberships>>::build(pg, &tx);
            let orgs = <_ as RepoFor<domain::Orgs>>::build(pg, &tx);
            let audit = <_ as RepoFor<domain::Audit>>::build(pg, &tx);
            async move {
                OrgSettings::new(Whoami::new(sessions, memberships), orgs, NoPhotos, audit)
                    .rename(token, org, &name)
                    .await
            }
        },
    )
    .await
}

#[tokio::test]
async fn renaming_an_organisation_through_the_real_repositories_leaves_exactly_one_entry() {
    let Some(pool) = control_plane("audit_rename").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "Acme", "owner@example.com").await;
    let token = backend::sessions(&pg)
        .issue(owner, Some(org), None)
        .await
        .expect("a session");

    let card = rename_through_the_repositories(&pg, &token, org, "Acme Inc")
        .await
        .expect("an owner renames");

    let page = backend::audit(&pg)
        .list(org, &AuditFilter::default(), PageSize::default())
        .await
        .expect("the log");
    assert_eq!(&*card.name, "Acme Inc");
    assert_eq!(page.entries.len(), 1);
    let logged = &page.entries[0];
    assert_eq!(logged.entry.action, Action::OrgRenamed);
    assert_eq!(logged.entry.actor, Actor::Account(owner));
    assert_eq!(logged.label.as_deref(), Some("owner@example.com"));
    assert_eq!(logged.entry.detail.get("from"), Some(Some("Acme")));
    assert_eq!(logged.entry.detail.get("to"), Some(Some("Acme Inc")));
}

#[tokio::test]
async fn a_rename_refused_to_a_member_leaves_no_entry_and_the_old_name() {
    let Some(pool) = control_plane("audit_refused_rename").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, _owner) = an_org(&pool, "Acme", "owner@example.com").await;
    let member = an_account(&pool, "member@example.com").await;
    backend::memberships(&pg)
        .grant(org, "member@example.com", OrgRole::Member)
        .await
        .expect("a member");
    let token = backend::sessions(&pg)
        .issue(member, Some(org), None)
        .await
        .expect("a session");

    let refused = rename_through_the_repositories(&pg, &token, org, "Mine Now").await;

    let page = backend::audit(&pg)
        .list(org, &AuditFilter::default(), PageSize::default())
        .await
        .expect("the log");
    let kept = backend::orgs(&pg)
        .only(org)
        .await
        .expect("the organisation")
        .expect("still there");
    assert_eq!(refused.err(), Some(SettingError::NotYoursToChange));
    assert!(page.entries.is_empty());
    assert_eq!(&*kept.name, "Acme");
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
fn no_statement_in_the_application_updates_or_deletes_an_audit_entry() {
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
                "update audit_entries",
                "delete from audit_entries",
                "truncate audit_entries",
            ]
            .iter()
            .any(|statement| text.contains(statement))
        })
        .collect();

    assert!(offending.is_empty(), "{offending:?}");
}
