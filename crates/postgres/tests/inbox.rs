use ferristrace_domain::{
    AccountId, Channels, InboxItemDraft, InboxItemKind, InboxStore, NotificationPreferences,
    NotificationSettings, TenantId,
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

fn at(pool: &PgPool, instant: u64) -> Postgres<FixedClock> {
    Postgres::new(pool.clone(), FixedClock::at(instant))
}

async fn an_org(pool: &PgPool, name: &str, owner_email: &str) -> (TenantId, AccountId) {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO accounts (id, email) VALUES ($1, $2)")
        .bind(id)
        .bind(owner_email)
        .execute(pool)
        .await
        .expect("an account");
    let org = backend::memberships(&at(pool, NOW))
        .found(name, owner_email)
        .await
        .expect("the organisation is founded");
    (org, AccountId::new(id))
}

fn draft(account: AccountId, org: TenantId, kind: InboxItemKind) -> InboxItemDraft {
    InboxItemDraft {
        account,
        org,
        kind,
        title: "A new exception type".into(),
        body: "checkout-api started raising TimeoutError".into(),
        link: Some("/issues/abc123".into()),
    }
}

#[tokio::test]
async fn a_raised_item_is_listed_unread_newest_first() {
    let Some(pool) = control_plane("inbox_round_trip").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let (org, account) = an_org(&pool, "Acme", "person@example.com").await;

    let (first, second) = committed!(pg, |inbox: Inbox| {
        let first = inbox
            .create(&draft(account, org, InboxItemKind::Issue), 1_000)
            .await
            .expect("kept");
        let second = inbox
            .create(&draft(account, org, InboxItemKind::Usage), 2_000)
            .await
            .expect("kept");
        (first, second)
    });

    let listed = committed!(pg, |inbox: Inbox| inbox
        .list(account, org, 10)
        .await
        .expect("read"));

    assert_eq!(listed, vec![second.clone(), first.clone()]);
    assert_eq!(first.read_ms, None);
    assert_eq!(first.link.as_deref(), Some("/issues/abc123"));
}

#[tokio::test]
async fn listing_never_crosses_accounts_or_organisations() {
    let Some(pool) = control_plane("inbox_scoped").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let (my_org, mine) = an_org(&pool, "Acme", "me@example.com").await;
    let (other_org, theirs) = an_org(&pool, "Globex", "them@example.com").await;

    committed!(pg, |inbox: Inbox| {
        inbox
            .create(&draft(mine, my_org, InboxItemKind::Issue), 1_000)
            .await
            .expect("kept");
        inbox
            .create(&draft(theirs, other_org, InboxItemKind::Issue), 1_000)
            .await
            .expect("kept");
    });

    let mine_only = committed!(pg, |inbox: Inbox| inbox
        .list(mine, my_org, 10)
        .await
        .expect("read"));
    let wrong_org = committed!(pg, |inbox: Inbox| inbox
        .list(mine, other_org, 10)
        .await
        .expect("read"));

    assert_eq!(mine_only.len(), 1);
    assert_eq!(mine_only[0].account, mine);
    assert!(wrong_org.is_empty());
}

#[tokio::test]
async fn marking_read_is_scoped_and_idempotent_and_an_unknown_id_is_not_found() {
    let Some(pool) = control_plane("inbox_mark_read").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let (org, account) = an_org(&pool, "Acme", "person@example.com").await;
    let raised = committed!(pg, |inbox: Inbox| inbox
        .create(&draft(account, org, InboxItemKind::Membership), 1_000)
        .await
        .expect("kept"));

    let first = committed!(pg, |inbox: Inbox| inbox
        .mark_read(account, org, raised.id, 2_000)
        .await
        .expect("marked"));
    let again = committed!(pg, |inbox: Inbox| inbox
        .mark_read(account, org, raised.id, 3_000)
        .await
        .expect("marked"));
    let unknown = committed!(pg, |inbox: Inbox| inbox
        .mark_read(
            account,
            org,
            ferristrace_domain::InboxItemId::fresh(),
            2_000
        )
        .await
        .expect("checked"));

    assert!(first, "the item existed, so marking it read succeeds");
    assert!(
        again,
        "marking an already-read item read is a no-op success"
    );
    assert!(!unknown, "an id nobody raised is not found");
    let listed = committed!(pg, |inbox: Inbox| inbox
        .list(account, org, 10)
        .await
        .expect("read"));
    assert_eq!(
        listed[0].read_ms,
        Some(2_000),
        "the first mark is the one that stuck"
    );
}

#[tokio::test]
async fn mark_all_read_counts_only_what_it_changed() {
    let Some(pool) = control_plane("inbox_mark_all").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let (org, account) = an_org(&pool, "Acme", "person@example.com").await;
    committed!(pg, |inbox: Inbox| {
        inbox
            .create(&draft(account, org, InboxItemKind::Issue), 1_000)
            .await
            .expect("kept");
        inbox
            .create(&draft(account, org, InboxItemKind::Usage), 1_000)
            .await
            .expect("kept");
    });

    let flipped = committed!(pg, |inbox: Inbox| inbox
        .mark_all_read(account, org, 5_000)
        .await
        .expect("marked"));
    let again = committed!(pg, |inbox: Inbox| inbox
        .mark_all_read(account, org, 6_000)
        .await
        .expect("marked"));

    assert_eq!(flipped, 2);
    assert_eq!(again, 0, "nothing was left unread the second time");
}

#[tokio::test]
async fn pruning_drops_only_what_is_older_than_the_cutoff() {
    let Some(pool) = control_plane("inbox_prune").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let (org, account) = an_org(&pool, "Acme", "person@example.com").await;
    committed!(pg, |inbox: Inbox| {
        inbox
            .create(&draft(account, org, InboxItemKind::Issue), 1_000)
            .await
            .expect("kept");
        inbox
            .create(&draft(account, org, InboxItemKind::Usage), 9_000)
            .await
            .expect("kept");
    });

    let pruned = committed!(pg, |inbox: Inbox| inbox.prune(5_000).await.expect("pruned"));

    assert_eq!(pruned, 1);
    let left = committed!(pg, |inbox: Inbox| inbox
        .list(account, org, 10)
        .await
        .expect("read"));
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].created_ms, 9_000);
}

#[tokio::test]
async fn an_account_with_no_notification_preferences_set_reads_the_defaults() {
    let Some(pool) = control_plane("inbox_notify_prefs_defaults").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let (_org, account) = an_org(&pool, "Acme", "person@example.com").await;

    let preferences = committed!(pg, |inbox: Inbox| inbox
        .of(account)
        .await
        .expect("read")
        .expect("the account exists"));

    assert_eq!(preferences, NotificationPreferences::default());
}

#[tokio::test]
async fn chosen_notification_preferences_travel_through_postgres() {
    let Some(pool) = control_plane("inbox_notify_prefs_round_trip").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let (_org, account) = an_org(&pool, "Acme", "person@example.com").await;
    let mut chosen = NotificationPreferences {
        email_enabled: false,
        weekly_digest: true,
        ..NotificationPreferences::default()
    };
    chosen.channels.insert(
        InboxItemKind::Issue,
        Channels {
            in_app: true,
            email: false,
        },
    );

    committed!(pg, |inbox: Inbox| inbox
        .set(account, &chosen)
        .await
        .expect("kept"));
    let read_back = committed!(pg, |inbox: Inbox| inbox
        .of(account)
        .await
        .expect("read")
        .expect("the account exists"));

    assert_eq!(read_back, chosen);
}
