use ferristrace_application::repository;
use ferristrace_domain::{
    Action, Actor, AuditEntry, AuditFilter, AuditPage, AuditTrail, Clock, Detail, EntryId, Logged,
    PageSize, StoreError, Target, TargetKind, TenantId,
};
use sqlx::Row;
use sqlx::postgres::PgRow;
use uuid::Uuid;

use crate::keys::to_nanos;
use crate::unit_of_work::{Postgres, SharedTx};

#[repository(domain = Audit, backend = Postgres<C>, generics = <C: Clock + Clone + Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresAuditTrail<C> {
    tx: SharedTx,
    clock: C,
}

impl<C: Clock + Clone> PostgresAuditTrail<C> {
    pub fn new(backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self {
            tx: tx.clone(),
            clock: backend.clock().clone(),
        }
    }
}

fn unreachable(doing: &'static str) -> impl FnOnce(sqlx::Error) -> StoreError {
    move |error| {
        tracing::error!(%error, "the control plane refused to {doing}");
        StoreError::Unreachable
    }
}

fn refused(what: &str) -> StoreError {
    tracing::error!(
        what,
        "a stored audit entry carries a value the domain refuses"
    );
    StoreError::Unreachable
}

fn read_entry(org: TenantId, row: &PgRow) -> Result<Logged, StoreError> {
    let actor_kind: String = row.get("actor_kind");
    let actor = Actor::of(&actor_kind, row.get("actor_id")).ok_or_else(|| refused("actor"))?;
    let action: String = row.get("action");
    let action = Action::parse(&action).ok_or_else(|| refused("action"))?;
    let target_kind: String = row.get("target_kind");
    let target_kind = TargetKind::parse(&target_kind).ok_or_else(|| refused("target"))?;
    let keys: Vec<String> = row.get("detail_keys");
    let values: Vec<Option<String>> = row.get("detail_values");
    let detail = Detail::from_pairs(
        keys.into_iter()
            .zip(values)
            .map(|(key, value)| (key.into(), value.map(Into::into)))
            .collect(),
    );
    Ok(Logged {
        id: EntryId::new(row.get("id")),
        at_unix_nanos: u64::try_from(row.get::<i64, _>("at")).unwrap_or_default(),
        entry: AuditEntry {
            org,
            actor,
            action,
            target: Target::of(
                target_kind,
                row.get::<Option<String>, _>("target_id").map(Into::into),
            ),
            detail,
        },
        label: row.get::<Option<String>, _>("label").map(Into::into),
    })
}

impl<C: Clock + Clone + Send + Sync> AuditTrail for PostgresAuditTrail<C> {
    async fn record(&self, entry: &AuditEntry) -> Result<(), StoreError> {
        let keys: Vec<&str> = entry.detail.pairs().iter().map(|(key, _)| &**key).collect();
        let values: Vec<Option<&str>> = entry
            .detail
            .pairs()
            .iter()
            .map(|(_, value)| value.as_deref())
            .collect();
        let mut tx = self.tx.lock().await;
        sqlx::query(
            "INSERT INTO audit_entries \
             (id, org_id, actor_kind, actor_id, action, target_kind, target_id, detail, at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, jsonb_object($8::text[], $9::text[]), \
             GREATEST($10, COALESCE( \
             (SELECT max(at) FROM audit_entries WHERE org_id = $2), 0) + 1))",
        )
        .bind(Uuid::new_v4())
        .bind(entry.org.as_uuid())
        .bind(entry.actor.kind())
        .bind(entry.actor.id())
        .bind(entry.action.as_str())
        .bind(entry.target.kind.as_str())
        .bind(entry.target.id.as_deref())
        .bind(keys)
        .bind(values)
        .bind(to_nanos(self.clock.now()))
        .execute(&mut **tx)
        .await
        .map_err(unreachable("record an audit entry"))?;
        Ok(())
    }

    async fn list(
        &self,
        org: TenantId,
        filter: &AuditFilter,
        page: PageSize,
    ) -> Result<AuditPage, StoreError> {
        let before_at = filter
            .before
            .map(|cursor| i64::try_from(cursor.at_unix_nanos).unwrap_or(i64::MAX));
        let before_id = filter.before.map(|cursor| cursor.id.as_uuid());
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(
            "SELECT e.id AS id, e.actor_kind AS actor_kind, e.actor_id AS actor_id, \
             e.action AS action, e.target_kind AS target_kind, e.target_id AS target_id, \
             e.at AS at, \
             ARRAY(SELECT d.key FROM jsonb_each_text(e.detail) d ORDER BY d.key) AS detail_keys, \
             ARRAY(SELECT d.value FROM jsonb_each_text(e.detail) d ORDER BY d.key) AS detail_values, \
             COALESCE(a.email, NULLIF(k.name, ''), NULLIF(g.name, '')) AS label \
             FROM audit_entries e \
             LEFT JOIN accounts a ON e.actor_kind = 'account' AND a.id = e.actor_id \
             LEFT JOIN api_keys k ON e.actor_kind = 'api_key' AND k.id = e.actor_id \
             AND k.org_id = e.org_id \
             LEFT JOIN agents g ON e.actor_kind = 'agent' AND g.id = e.actor_id \
             AND g.org_id = e.org_id \
             WHERE e.org_id = $1 \
             AND ($2::text IS NULL OR e.actor_kind = $2) \
             AND ($3::uuid IS NULL OR e.actor_id = $3) \
             AND ($4::text IS NULL OR e.action = $4) \
             AND ($5::bigint IS NULL OR (e.at, e.id) < ($5, $6::uuid)) \
             ORDER BY e.at DESC, e.id DESC \
             LIMIT $7",
        )
        .bind(org.as_uuid())
        .bind(filter.actor.map(Actor::kind))
        .bind(filter.actor.and_then(Actor::id))
        .bind(filter.action.map(Action::as_str))
        .bind(before_at)
        .bind(before_id)
        .bind(i64::from(page.probe()))
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("list the audit entries"))?;
        let probed = rows
            .iter()
            .map(|row| read_entry(org, row))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(AuditPage::cut(probed, page))
    }
}
