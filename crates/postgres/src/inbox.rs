use std::collections::BTreeMap;

use ferristrace_application::repository;
use ferristrace_domain::{
    AccountId, Channels, InboxItem, InboxItemDraft, InboxItemId, InboxItemKind, InboxStore, KINDS,
    NotificationPreferences, NotificationSettings, StoreError, TenantId,
};
use serde_json::{Map, Value, json};
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

fn encode_channels(channels: &BTreeMap<InboxItemKind, Channels>) -> String {
    let map: Map<String, Value> = channels
        .iter()
        .map(|(kind, held)| {
            (
                kind.as_str().to_owned(),
                json!({ "in_app": held.in_app, "email": held.email }),
            )
        })
        .collect();
    Value::Object(map).to_string()
}

fn decode_channels(text: &str) -> BTreeMap<InboxItemKind, Channels> {
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(text) else {
        return BTreeMap::new();
    };
    map.into_iter()
        .filter_map(|(key, value)| {
            let kind = InboxItemKind::parse(&key)?;
            let in_app = value.get("in_app")?.as_bool()?;
            let email = value.get("email")?.as_bool()?;
            Some((kind, Channels { in_app, email }))
        })
        .collect()
}

fn read_preferences(row: &PgRow) -> NotificationPreferences {
    let chosen = row
        .get::<Option<String>, _>("notify_channels")
        .map(|text| decode_channels(&text))
        .unwrap_or_default();
    NotificationPreferences {
        email_enabled: row
            .get::<Option<bool>, _>("notify_email_enabled")
            .unwrap_or(true),
        weekly_digest: row
            .get::<Option<bool>, _>("notify_weekly_digest")
            .unwrap_or(false),
        channels: KINDS
            .into_iter()
            .map(|kind| {
                (
                    kind,
                    chosen
                        .get(&kind)
                        .copied()
                        .unwrap_or_else(|| kind.default_channels()),
                )
            })
            .collect(),
    }
}

#[repository(domain = Inbox, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug, Clone)]
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

impl NotificationSettings for PostgresInbox {
    async fn of(&self, account: AccountId) -> Result<Option<NotificationPreferences>, StoreError> {
        let mut tx = self.tx.lock().await;
        let row = sqlx::query(
            "SELECT notify_email_enabled, notify_weekly_digest, notify_channels::text AS notify_channels \
             FROM accounts WHERE id = $1",
        )
        .bind(account.as_uuid())
        .fetch_optional(&mut **tx)
        .await
        .map_err(unreachable("read notification preferences"))?;
        Ok(row.as_ref().map(read_preferences))
    }

    async fn set(
        &self,
        account: AccountId,
        preferences: &NotificationPreferences,
    ) -> Result<(), StoreError> {
        let mut tx = self.tx.lock().await;
        sqlx::query(
            "UPDATE accounts SET notify_email_enabled = $2, notify_weekly_digest = $3, \
             notify_channels = $4::jsonb WHERE id = $1",
        )
        .bind(account.as_uuid())
        .bind(preferences.email_enabled)
        .bind(preferences.weekly_digest)
        .bind(encode_channels(&preferences.channels))
        .execute(&mut **tx)
        .await
        .map_err(unreachable("keep notification preferences"))?;
        Ok(())
    }
}
