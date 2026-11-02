use std::sync::{Arc, Mutex};
use uuid::Uuid;

use super::entities::Membership;
use super::errors::{SwitchError, WhoamiError};
use super::ports::Memberships;
use super::switching::Switching;
use super::value_objects::OrgRole;
use crate::clock::UnixSeconds;
use crate::identity::{
    AccountId, Agent, Email, Session, SessionError, SessionStore, SessionSwitch,
};
use crate::secret::SessionToken;
use crate::telemetry::TenantId;

const TOKEN: &str = "ft_ss_live";
const MINE: u128 = 1;
const ALSO_MINE: u128 = 2;
const THEIRS: u128 = 3;

fn an_org(seed: u128) -> TenantId {
    TenantId::new(Uuid::from_u128(seed))
}

fn a_token() -> SessionToken {
    SessionToken::from_plaintext(TOKEN)
}

#[derive(Debug, Clone, Default)]
struct OneSession {
    switched: Arc<Mutex<Vec<TenantId>>>,
    unreachable: bool,
}

impl SessionStore for OneSession {
    async fn issue(
        &self,
        _account: AccountId,
        _org: Option<TenantId>,
        _from: Option<&Agent>,
    ) -> Result<SessionToken, SessionError> {
        Ok(a_token())
    }

    async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
        if token.exposed() != TOKEN {
            return Err(SessionError::Unknown);
        }
        Ok(Session {
            account: AccountId::new(Uuid::from_u128(9)),
            email: Email::try_from("person@example.com").expect("an address"),
            name: None,
            photo: None,
            org: None,
            expires_at: UnixSeconds::new(0),
        })
    }

    async fn revoke(&self, _token: &SessionToken) -> Result<(), SessionError> {
        Ok(())
    }
}

impl SessionSwitch for OneSession {
    async fn switch(&self, _token: &SessionToken, org: TenantId) -> Result<(), SessionError> {
        if self.unreachable {
            return Err(SessionError::StoreUnreachable);
        }
        self.switched.lock().expect("the lock").push(org);
        Ok(())
    }
}

struct Belonging(Vec<(u128, OrgRole)>);

impl Memberships for Belonging {
    async fn of(&self, _account: AccountId) -> Result<Vec<Membership>, WhoamiError> {
        Ok(self
            .0
            .iter()
            .map(|(seed, role)| Membership {
                org: an_org(*seed),
                name: "Acme".into(),
                role: *role,
            })
            .collect())
    }
}

fn belonging_to_two() -> Belonging {
    Belonging(vec![(MINE, OrgRole::Owner), (ALSO_MINE, OrgRole::Member)])
}

#[tokio::test]
async fn a_member_of_any_role_switches_to_the_organisation() {
    let sessions = OneSession::default();
    let switching = Switching::new(sessions.clone(), belonging_to_two());

    switching
        .to(&a_token(), an_org(ALSO_MINE))
        .await
        .expect("a member switches");

    assert_eq!(
        *sessions.switched.lock().expect("the lock"),
        vec![an_org(ALSO_MINE)]
    );
}

#[tokio::test]
async fn an_organisation_the_account_does_not_belong_to_is_refused_and_nothing_is_written() {
    let sessions = OneSession::default();
    let switching = Switching::new(sessions.clone(), belonging_to_two());

    let refused = switching.to(&a_token(), an_org(THEIRS)).await;

    assert_eq!(refused, Err(SwitchError::NotYours));
    assert!(sessions.switched.lock().expect("the lock").is_empty());
}

#[tokio::test]
async fn an_unknown_token_is_refused() {
    let switching = Switching::new(OneSession::default(), belonging_to_two());

    let refused = switching
        .to(&SessionToken::from_plaintext("ft_ss_other"), an_org(MINE))
        .await;

    assert_eq!(refused, Err(SwitchError::NoSession));
}

#[tokio::test]
async fn a_session_store_that_cannot_write_is_reported() {
    let sessions = OneSession {
        unreachable: true,
        ..OneSession::default()
    };
    let switching = Switching::new(sessions, belonging_to_two());

    let refused = switching.to(&a_token(), an_org(MINE)).await;

    assert_eq!(refused, Err(SwitchError::StoreUnreachable));
}
