use core::fmt;
use core::future::Future;

use crate::credential::Email;
use crate::identity::AccountId;
use crate::photo::Portrait;
use crate::secret::SessionToken;
use crate::session::{SessionError, SessionStore};
use crate::tenant::TenantId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OrgRole {
    Member,
    Admin,
    Owner,
}

impl OrgRole {
    pub fn at_least(self, floor: Self) -> bool {
        self >= floor
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Member => "member",
            Self::Admin => "admin",
            Self::Owner => "owner",
        }
    }
}

impl fmt::Display for OrgRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Membership {
    pub org: TenantId,
    pub name: Box<str>,
    pub role: OrgRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WhoamiError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("the control plane could not be reached")]
    StoreUnreachable,
}

impl From<SessionError> for WhoamiError {
    fn from(error: SessionError) -> Self {
        match error {
            SessionError::Unknown => Self::NoSession,
            SessionError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

pub trait Memberships {
    fn of(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Vec<Membership>, WhoamiError>> + Send;
}

impl<T: Memberships + Sync> Memberships for &T {
    fn of(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Vec<Membership>, WhoamiError>> + Send {
        (*self).of(account)
    }
}

impl<T: Memberships + Send + Sync> Memberships for std::sync::Arc<T> {
    fn of(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Vec<Membership>, WhoamiError>> + Send {
        self.as_ref().of(account)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Who {
    pub account: AccountId,
    pub email: Email,
    pub name: Option<Box<str>>,
    pub photo: Option<Portrait>,
    pub active_org: Option<TenantId>,
    pub memberships: Vec<Membership>,
}

impl Who {
    pub fn role_in(&self, org: TenantId) -> Option<OrgRole> {
        self.memberships
            .iter()
            .find(|membership| membership.org == org)
            .map(|membership| membership.role)
    }

    pub fn may_act_in(&self, org: TenantId, floor: OrgRole) -> bool {
        self.role_in(org).is_some_and(|role| role.at_least(floor))
    }
}

#[derive(Debug)]
pub struct Whoami<S, G> {
    sessions: S,
    memberships: G,
}

impl<S, G> Whoami<S, G>
where
    S: SessionStore,
    G: Memberships,
{
    pub fn new(sessions: S, memberships: G) -> Self {
        Self {
            sessions,
            memberships,
        }
    }

    pub async fn of(&self, token: &SessionToken) -> Result<Who, WhoamiError> {
        let session = self.sessions.resolve(token).await?;
        let memberships = self.memberships.of(session.account).await?;
        Ok(Who {
            account: session.account,
            email: session.email,
            name: session.name,
            photo: session.photo,
            active_org: session.org,
            memberships,
        })
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{Membership, OrgRole, Who};
    use crate::credential::Email;
    use crate::identity::AccountId;
    use crate::tenant::TenantId;

    fn an_org(seed: u128) -> TenantId {
        TenantId::new(Uuid::from_u128(seed))
    }

    fn a_membership(org: TenantId, role: OrgRole) -> Membership {
        Membership {
            org,
            name: "Acme".into(),
            role,
        }
    }

    fn who(memberships: Vec<Membership>) -> Who {
        Who {
            account: AccountId::new(Uuid::from_u128(1)),
            email: Email::try_from("person@example.com").expect("an address"),
            name: None,
            photo: None,
            active_org: None,
            memberships,
        }
    }

    #[test]
    fn an_owner_outranks_an_admin_who_outranks_a_member() {
        assert!(OrgRole::Owner.at_least(OrgRole::Admin));
        assert!(OrgRole::Admin.at_least(OrgRole::Member));
        assert!(OrgRole::Member.at_least(OrgRole::Member));
        assert!(!OrgRole::Member.at_least(OrgRole::Admin));
        assert!(!OrgRole::Admin.at_least(OrgRole::Owner));
    }

    #[test]
    fn a_role_names_itself_as_the_schema_spells_it() {
        assert_eq!(OrgRole::Owner.to_string(), "owner");
        assert_eq!(OrgRole::Admin.to_string(), "admin");
        assert_eq!(OrgRole::Member.to_string(), "member");
    }

    #[test]
    fn a_role_is_read_from_the_organisation_it_belongs_to() {
        let person = who(vec![
            a_membership(an_org(1), OrgRole::Owner),
            a_membership(an_org(2), OrgRole::Member),
        ]);

        assert_eq!(person.role_in(an_org(1)), Some(OrgRole::Owner));
        assert_eq!(person.role_in(an_org(2)), Some(OrgRole::Member));
        assert_eq!(person.role_in(an_org(3)), None);
    }

    #[test]
    fn acting_needs_a_membership_in_that_very_organisation() {
        let person = who(vec![a_membership(an_org(1), OrgRole::Owner)]);

        assert!(person.may_act_in(an_org(1), OrgRole::Owner));
        assert!(
            !person.may_act_in(an_org(2), OrgRole::Member),
            "owning one organisation grants nothing in another"
        );
    }

    #[test]
    fn a_member_cannot_act_where_an_admin_is_required() {
        let person = who(vec![a_membership(an_org(1), OrgRole::Member)]);

        assert!(person.may_act_in(an_org(1), OrgRole::Member));
        assert!(!person.may_act_in(an_org(1), OrgRole::Admin));
    }
}
