use ferristrace_application::repository;
use ferristrace_domain::{
    AccountId, InboxItem, InboxItemDraft, InboxItemId, InboxItemKind, InboxStore, StoreError,
    TenantId,
};
use sqlx::Row;
use sqlx::postgres::PgRow;
use uuid::Uuid;

use crate::unit_of_work::{Postgres, SharedTx};

fn unreachable(doing: &'static str) -> impl FnOnce(sqlx::Error) -> StoreError {
    move |error| {
        tracing::error!(%error, "the control plane refused to {doing}");
        StoreError::Unreachable
    }
}

fn refused(what: &str) -> StoreError {
    tracing::error!(
        what,
        "a stored inbox row carries a value the domain refuses"
    );
    StoreError::Unreachable
}

fn unsigned(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

fn read_item(row: &PgRow) -> Result<InboxItem, StoreError> {
    let kind: String = row.get("kind");
    let title: String = row.get("title");
    let body: String = row.get("body");
    Ok(InboxItem {
        id: InboxItemId::new(row.get("id")),
        account: AccountId::new(row.get("account_id")),
        org: TenantId::new(row.get("org_id")),
        kind: InboxItemKind::parse(&kind).ok_or_else(|| refused("kind"))?,
        title: title.into(),
        body: body.into(),
        link: row.get::<Option<String>, _>("link").map(Into::into),
        created_ms: unsigned(row.get("created_at")),
        read_ms: row.get::<Option<i64>, _>("read_at").map(unsigned),
    })
}

#[repository(domain = Inbox, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresInbox {
    tx: SharedTx,
}

impl PostgresInbox {
    pub fn new<C>(_backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self { tx: tx.clone() }
    }
}

impl InboxStore for PostgresInbox {
    async fn list(
        &self,
        account: AccountId,
        org: TenantId,
        limit: u32,
    ) -> Result<Vec<InboxItem>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(
            "SELECT id, account_id, org_id, kind, title, body, link, created_at, read_at \
             FROM inbox_items WHERE account_id = $1 AND org_id = $2 \
             ORDER BY created_at DESC, id LIMIT $3",
        )
        .bind(account.as_uuid())
        .bind(org.as_uuid())
        .bind(i64::from(limit))
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("list the inbox"))?;
        rows.iter().map(read_item).collect()
    }

    async fn create(&self, draft: &InboxItemDraft, at_ms: u64) -> Result<InboxItem, StoreError> {
        let mut tx = self.tx.lock().await;
        let row = sqlx::query(
            "INSERT INTO inbox_items \
             (id, account_id, org_id, kind, title, body, link, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             RETURNING id, account_id, org_id, kind, title, body, link, created_at, read_at",
        )
        .bind(Uuid::new_v4())
        .bind(draft.account.as_uuid())
        .bind(draft.org.as_uuid())
        .bind(draft.kind.as_str())
        .bind(&*draft.title)
        .bind(&*draft.body)
        .bind(draft.link.as_deref())
        .bind(i64::try_from(at_ms).unwrap_or(i64::MAX))
        .fetch_one(&mut **tx)
        .await
        .map_err(unreachable("raise a notification"))?;
        read_item(&row)
    }

    async fn mark_read(
        &self,
        account: AccountId,
        org: TenantId,
        id: InboxItemId,
        at_ms: u64,
    ) -> Result<bool, StoreError> {
        let mut tx = self.tx.lock().await;
        let done = sqlx::query(
            "UPDATE inbox_items SET read_at = $4 \
             WHERE id = $1 AND account_id = $2 AND org_id = $3 AND read_at IS NULL",
        )
        .bind(id.as_uuid())
        .bind(account.as_uuid())
        .bind(org.as_uuid())
        .bind(i64::try_from(at_ms).unwrap_or(i64::MAX))
        .execute(&mut **tx)
        .await
        .map_err(unreachable("mark a notification read"))?;
        if done.rows_affected() == 1 {
            return Ok(true);
        }
        let exists = sqlx::query(
            "SELECT 1 AS present FROM inbox_items WHERE id = $1 AND account_id = $2 AND org_id = $3",
        )
        .bind(id.as_uuid())
        .bind(account.as_uuid())
        .bind(org.as_uuid())
        .fetch_optional(&mut **tx)
        .await
        .map_err(unreachable("read a notification"))?;
        Ok(exists.is_some())
    }

    async fn mark_all_read(
        &self,
        account: AccountId,
        org: TenantId,
        at_ms: u64,
    ) -> Result<u64, StoreError> {
        let mut tx = self.tx.lock().await;
        let done = sqlx::query(
            "UPDATE inbox_items SET read_at = $3 \
             WHERE account_id = $1 AND org_id = $2 AND read_at IS NULL",
        )
        .bind(account.as_uuid())
        .bind(org.as_uuid())
        .bind(i64::try_from(at_ms).unwrap_or(i64::MAX))
        .execute(&mut **tx)
        .await
        .map_err(unreachable("mark the inbox read"))?;
        Ok(done.rows_affected())
    }

    async fn prune(&self, older_than_ms: u64) -> Result<u64, StoreError> {
        let mut tx = self.tx.lock().await;
        let done = sqlx::query("DELETE FROM inbox_items WHERE created_at < $1")
            .bind(i64::try_from(older_than_ms).unwrap_or(i64::MAX))
            .execute(&mut **tx)
            .await
            .map_err(unreachable("prune the inbox"))?;
        Ok(done.rows_affected())
    }
}
