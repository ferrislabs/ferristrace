use std::sync::{Arc, Mutex};

use uuid::Uuid;

use crate::audit::{
    Actor, AuditEntry, AuditFilter, AuditPage, AuditTrail, EntryId, Logged, PageSize,
};
use crate::store::StoreError;
use crate::telemetry::IngestKey;
use crate::telemetry::{TenantDirectory, TenantId, UnknownKey};

pub async fn a_directory_resolves_the_key_it_holds<D: TenantDirectory>(
    directory: &D,
    held: &IngestKey,
    tenant: TenantId,
) {
    let resolved = directory.resolve(held).await;

    assert_eq!(resolved.map(|scope| scope.tenant), Ok(tenant));
}

pub async fn a_directory_refuses_a_key_it_does_not_hold<D: TenantDirectory>(
    directory: &D,
    absent: &IngestKey,
) {
    let resolved = directory.resolve(absent).await;

    assert_eq!(resolved.map(|scope| scope.tenant), Err(UnknownKey));
}

pub async fn a_directory_answers_the_same_key_the_same_way_twice<D: TenantDirectory>(
    directory: &D,
    held: &IngestKey,
) {
    let once = directory.resolve(held).await;
    let twice = directory.resolve(held).await;

    assert_eq!(once, twice);
}

#[derive(Debug)]
struct Labelled {
    actor: Actor,
    label: Box<str>,
}

#[derive(Debug, Clone, Default)]
pub struct Notebook {
    logged: Arc<Mutex<Vec<Logged>>>,
    labels: Arc<Mutex<Vec<Labelled>>>,
}

impl Notebook {
    pub fn entries(&self) -> Vec<AuditEntry> {
        self.logged
            .lock()
            .expect("the notebook lock")
            .iter()
            .map(|logged| logged.entry.clone())
            .collect()
    }

    pub fn label(&self, actor: Actor, label: &str) {
        self.labels
            .lock()
            .expect("the notebook lock")
            .push(Labelled {
                actor,
                label: label.into(),
            });
    }
}

impl AuditTrail for Notebook {
    async fn record(&self, entry: &AuditEntry) -> Result<(), StoreError> {
        let mut logged = self.logged.lock().expect("the notebook lock");
        let seed = u64::try_from(logged.len()).unwrap_or(u64::MAX) + 1;
        let label = self
            .labels
            .lock()
            .expect("the notebook lock")
            .iter()
            .find(|held| held.actor == entry.actor)
            .map(|held| held.label.clone());
        logged.push(Logged {
            id: EntryId::new(Uuid::from_u128(u128::from(seed))),
            at_unix_nanos: seed,
            entry: entry.clone(),
            label,
        });
        Ok(())
    }

    async fn list(
        &self,
        org: TenantId,
        filter: &AuditFilter,
        page: PageSize,
    ) -> Result<AuditPage, StoreError> {
        let probed = self
            .logged
            .lock()
            .expect("the notebook lock")
            .iter()
            .rev()
            .filter(|logged| logged.entry.org == org)
            .filter(|logged| filter.actor.is_none_or(|actor| logged.entry.actor == actor))
            .filter(|logged| {
                filter
                    .action
                    .is_none_or(|action| logged.entry.action == action)
            })
            .filter(|logged| {
                filter.before.is_none_or(|before| {
                    (logged.at_unix_nanos, logged.id) < (before.at_unix_nanos, before.id)
                })
            })
            .take(usize::try_from(page.probe()).unwrap_or(usize::MAX))
            .cloned()
            .collect();
        Ok(AuditPage::cut(probed, page))
    }
}
