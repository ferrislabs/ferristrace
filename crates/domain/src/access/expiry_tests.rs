use std::sync::Mutex;
use uuid::Uuid;

use super::entities::{ApiKey, ExpiringKey, IssuedKey};
use super::expiry::{Expired, Expiry};
use super::ports::ApiKeys;
use super::value_objects::{
    ApiSecret, ExpiryThreshold, KeyId, KeyName, KeyScopes, KeyTtl, ReadGrant,
};
use crate::audit::{Action, Actor};
use crate::clock::UnixSeconds;
use crate::identity::{AccountId, Email};
use crate::organisation::{Holder, Joined, Member, OrgRole, Roll};
use crate::store::StoreError;
use crate::telemetry::TenantId;
use crate::testing::{AlertLedger, Notebook};

const ORG: u128 = 1;
const CREATOR: u128 = 2;
const ADMIN: u128 = 3;
const MEMBER: u128 = 4;
const KEY: u128 = 100;

fn org() -> TenantId {
    TenantId::new(Uuid::from_u128(ORG))
}

fn account(seed: u128) -> AccountId {
    AccountId::new(Uuid::from_u128(seed))
}

fn a_key(created_by: Option<AccountId>) -> ExpiringKey {
    ExpiringKey {
        id: KeyId::new(Uuid::from_u128(KEY)),
        org: org(),
        name: "ci".into(),
        created_by,
    }
}

fn a_member(seed: u128, role: OrgRole) -> Member {
    Member {
        account: account(seed),
        email: Email::try_from(format!("person{seed}@example.com").as_str()).expect("an address"),
        name: None,
        photo: None,
        role,
        joined_at_unix_nanos: 1,
    }
}

#[derive(Debug, Default)]
struct Vault {
    due_notice: Mutex<Vec<(ExpiringKey, ExpiryThreshold)>>,
    due_revoke: Mutex<Vec<ExpiringKey>>,
    notified: Mutex<Vec<(KeyId, ExpiryThreshold)>>,
    revoked: Mutex<Vec<KeyId>>,
}

impl ApiKeys for Vault {
    async fn list(&self, _org: TenantId) -> Result<Vec<ApiKey>, StoreError> {
        Ok(Vec::new())
    }

    async fn issue(
        &self,
        _org: TenantId,
        _project: Option<crate::catalogue::ProjectId>,
        _name: &KeyName,
        _scopes: &KeyScopes,
        _by: AccountId,
        _ttl: KeyTtl,
    ) -> Result<IssuedKey, StoreError> {
        Err(StoreError::Unreachable)
    }

    async fn revoke(&self, _org: TenantId, id: KeyId) -> Result<bool, StoreError> {
        self.revoked.lock().expect("the lock").push(id);
        Ok(true)
    }

    async fn grant_for(&self, _secret: &ApiSecret) -> Result<Option<ReadGrant>, StoreError> {
        Ok(None)
    }

    async fn due_for_expiry_notice(
        &self,
        _now_unix_nanos: u64,
    ) -> Result<Vec<(ExpiringKey, ExpiryThreshold)>, StoreError> {
        Ok(self.due_notice.lock().expect("the lock").clone())
    }

    async fn mark_expiry_notice_sent(
        &self,
        id: KeyId,
        threshold: ExpiryThreshold,
    ) -> Result<(), StoreError> {
        self.notified
            .lock()
            .expect("the lock")
            .push((id, threshold));
        Ok(())
    }

    async fn due_for_auto_revoke(
        &self,
        _now_unix_nanos: u64,
    ) -> Result<Vec<ExpiringKey>, StoreError> {
        Ok(self.due_revoke.lock().expect("the lock").clone())
    }
}

#[derive(Debug, Default)]
struct Admins(Vec<Member>);

impl Roll for Admins {
    async fn of(&self, _org: TenantId) -> Result<Vec<Member>, StoreError> {
        Ok(self.0.clone())
    }

    async fn grant(
        &self,
        _org: TenantId,
        _account: AccountId,
        _role: OrgRole,
    ) -> Result<Option<Member>, StoreError> {
        Ok(None)
    }

    async fn remove(&self, _org: TenantId, _account: AccountId) -> Result<bool, StoreError> {
        Ok(false)
    }

    async fn standing_of(&self, _email: &Email) -> Result<Option<Holder>, StoreError> {
        Ok(None)
    }

    async fn create_account(&self, _email: &Email) -> Result<AccountId, StoreError> {
        Ok(account(0))
    }

    async fn join(
        &self,
        _org: TenantId,
        _account: AccountId,
        _role: OrgRole,
    ) -> Result<Option<Joined>, StoreError> {
        Ok(None)
    }
}

fn expiry<'a>(
    vault: &'a Vault,
    roll: Admins,
    duty: &AlertLedger,
    notebook: &Notebook,
) -> Expiry<&'a Vault, Admins, AlertLedger, Notebook> {
    Expiry::new(vault, roll, duty.clone(), notebook.clone())
}

#[tokio::test]
async fn a_key_past_its_deadline_is_revoked_and_its_creator_is_told() {
    let vault = Vault::default();
    vault
        .due_revoke
        .lock()
        .expect("the lock")
        .push(a_key(Some(account(CREATOR))));
    let duty = AlertLedger::default();
    let notebook = Notebook::default();

    let result = expiry(&vault, Admins::default(), &duty, &notebook)
        .tick(UnixSeconds::new(1))
        .await
        .expect("the tick runs");

    let Expired::Worked { revoked, noticed } = result else {
        panic!("expected work to be done");
    };
    assert_eq!(revoked.len(), 1);
    assert_eq!(revoked[0].notify, vec![account(CREATOR)]);
    assert!(noticed.is_empty());
    assert_eq!(
        vault.revoked.lock().expect("the lock").clone(),
        vec![KeyId::new(Uuid::from_u128(KEY))]
    );
    let entries = notebook.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].action, Action::KeyRevoked);
    assert_eq!(entries[0].actor, Actor::System);
}

#[tokio::test]
async fn a_key_with_no_creator_falls_back_to_the_organisations_admins() {
    let vault = Vault::default();
    vault.due_revoke.lock().expect("the lock").push(a_key(None));
    let roll = Admins(vec![
        a_member(ADMIN, OrgRole::Admin),
        a_member(MEMBER, OrgRole::Member),
    ]);
    let duty = AlertLedger::default();
    let notebook = Notebook::default();

    let Expired::Worked { revoked, .. } = expiry(&vault, roll, &duty, &notebook)
        .tick(UnixSeconds::new(1))
        .await
        .expect("the tick runs")
    else {
        panic!("expected work to be done");
    };

    assert_eq!(
        revoked[0].notify,
        vec![account(ADMIN)],
        "not the plain member"
    );
}

#[tokio::test]
async fn a_key_entering_its_notice_window_is_marked_sent_and_notified() {
    let vault = Vault::default();
    vault
        .due_notice
        .lock()
        .expect("the lock")
        .push((a_key(Some(account(CREATOR))), ExpiryThreshold::Days7));
    let duty = AlertLedger::default();
    let notebook = Notebook::default();

    let Expired::Worked { noticed, revoked } = expiry(&vault, Admins::default(), &duty, &notebook)
        .tick(UnixSeconds::new(1))
        .await
        .expect("the tick runs")
    else {
        panic!("expected work to be done");
    };

    assert!(revoked.is_empty());
    assert_eq!(noticed.len(), 1);
    assert_eq!(noticed[0].threshold, ExpiryThreshold::Days7);
    assert_eq!(noticed[0].notify, vec![account(CREATOR)]);
    assert_eq!(
        vault.notified.lock().expect("the lock").clone(),
        vec![(KeyId::new(Uuid::from_u128(KEY)), ExpiryThreshold::Days7)]
    );
}

#[tokio::test]
async fn a_second_tick_in_the_same_slot_is_skipped() {
    let vault = Vault::default();
    vault
        .due_revoke
        .lock()
        .expect("the lock")
        .push(a_key(Some(account(CREATOR))));
    let duty = AlertLedger::default();
    let notebook = Notebook::default();
    let desk = expiry(&vault, Admins::default(), &duty, &notebook);

    desk.tick(UnixSeconds::new(1))
        .await
        .expect("the first tick runs");
    let second = desk
        .tick(UnixSeconds::new(2))
        .await
        .expect("the second tick runs");

    assert_eq!(second, Expired::Skipped);
    assert_eq!(
        vault.revoked.lock().expect("the lock").len(),
        1,
        "the key was only revoked once"
    );
}
