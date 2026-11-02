use core::future::Future;

use super::entities::{Holder, Joined, Member, Membership, Organisation, Team};
use super::errors::WhoamiError;
use super::value_objects::{Named, OrgName, OrgRole, TeamId, TeamName};
use crate::catalogue::ProjectId;
use crate::identity::{AccountId, Email};
use crate::profile::Portrait;
use crate::secret::SecretHash;
use crate::store::StoreError;
use crate::telemetry::TenantId;

pub trait Orgs {
    fn only(
        &self,
        org: TenantId,
    ) -> impl Future<Output = Result<Option<Organisation>, StoreError>> + Send;

    fn rename(
        &self,
        org: TenantId,
        name: &OrgName,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    fn set_logo(
        &self,
        org: TenantId,
        logo: Option<&Portrait>,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

impl<T: Orgs + Sync> Orgs for &T {
    fn only(
        &self,
        org: TenantId,
    ) -> impl Future<Output = Result<Option<Organisation>, StoreError>> + Send {
        (*self).only(org)
    }

    fn rename(
        &self,
        org: TenantId,
        name: &OrgName,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        (*self).rename(org, name)
    }

    fn set_logo(
        &self,
        org: TenantId,
        logo: Option<&Portrait>,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        (*self).set_logo(org, logo)
    }
}

impl<T: Orgs + Send + Sync> Orgs for std::sync::Arc<T> {
    fn only(
        &self,
        org: TenantId,
    ) -> impl Future<Output = Result<Option<Organisation>, StoreError>> + Send {
        self.as_ref().only(org)
    }

    fn rename(
        &self,
        org: TenantId,
        name: &OrgName,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        self.as_ref().rename(org, name)
    }

    fn set_logo(
        &self,
        org: TenantId,
        logo: Option<&Portrait>,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        self.as_ref().set_logo(org, logo)
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

pub trait Roll {
    fn of(&self, org: TenantId) -> impl Future<Output = Result<Vec<Member>, StoreError>> + Send;

    fn grant(
        &self,
        org: TenantId,
        account: AccountId,
        role: OrgRole,
    ) -> impl Future<Output = Result<Option<Member>, StoreError>> + Send;

    fn remove(
        &self,
        org: TenantId,
        account: AccountId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    fn standing_of(
        &self,
        email: &Email,
    ) -> impl Future<Output = Result<Option<Holder>, StoreError>> + Send;

    fn create_locked(
        &self,
        email: &Email,
    ) -> impl Future<Output = Result<AccountId, StoreError>> + Send;

    fn join(
        &self,
        org: TenantId,
        account: AccountId,
        role: OrgRole,
    ) -> impl Future<Output = Result<Option<Joined>, StoreError>> + Send;

    fn open_way_in(
        &self,
        account: AccountId,
        digest: SecretHash,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

impl<T: Roll + Sync> Roll for &T {
    fn of(&self, org: TenantId) -> impl Future<Output = Result<Vec<Member>, StoreError>> + Send {
        (*self).of(org)
    }

    fn grant(
        &self,
        org: TenantId,
        account: AccountId,
        role: OrgRole,
    ) -> impl Future<Output = Result<Option<Member>, StoreError>> + Send {
        (*self).grant(org, account, role)
    }

    fn remove(
        &self,
        org: TenantId,
        account: AccountId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        (*self).remove(org, account)
    }

    fn standing_of(
        &self,
        email: &Email,
    ) -> impl Future<Output = Result<Option<Holder>, StoreError>> + Send {
        (*self).standing_of(email)
    }

    fn create_locked(
        &self,
        email: &Email,
    ) -> impl Future<Output = Result<AccountId, StoreError>> + Send {
        (*self).create_locked(email)
    }

    fn join(
        &self,
        org: TenantId,
        account: AccountId,
        role: OrgRole,
    ) -> impl Future<Output = Result<Option<Joined>, StoreError>> + Send {
        (*self).join(org, account, role)
    }

    fn open_way_in(
        &self,
        account: AccountId,
        digest: SecretHash,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        (*self).open_way_in(account, digest)
    }
}

impl<T: Roll + Send + Sync> Roll for std::sync::Arc<T> {
    fn of(&self, org: TenantId) -> impl Future<Output = Result<Vec<Member>, StoreError>> + Send {
        self.as_ref().of(org)
    }

    fn grant(
        &self,
        org: TenantId,
        account: AccountId,
        role: OrgRole,
    ) -> impl Future<Output = Result<Option<Member>, StoreError>> + Send {
        self.as_ref().grant(org, account, role)
    }

    fn remove(
        &self,
        org: TenantId,
        account: AccountId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        self.as_ref().remove(org, account)
    }

    fn standing_of(
        &self,
        email: &Email,
    ) -> impl Future<Output = Result<Option<Holder>, StoreError>> + Send {
        self.as_ref().standing_of(email)
    }

    fn create_locked(
        &self,
        email: &Email,
    ) -> impl Future<Output = Result<AccountId, StoreError>> + Send {
        self.as_ref().create_locked(email)
    }

    fn join(
        &self,
        org: TenantId,
        account: AccountId,
        role: OrgRole,
    ) -> impl Future<Output = Result<Option<Joined>, StoreError>> + Send {
        self.as_ref().join(org, account, role)
    }

    fn open_way_in(
        &self,
        account: AccountId,
        digest: SecretHash,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        self.as_ref().open_way_in(account, digest)
    }
}

pub trait Squads {
    fn of(&self, org: TenantId) -> impl Future<Output = Result<Vec<Team>, StoreError>> + Send;

    fn create(
        &self,
        org: TenantId,
        name: &TeamName,
    ) -> impl Future<Output = Result<Named, StoreError>> + Send;

    fn rename(
        &self,
        org: TenantId,
        team: TeamId,
        name: &TeamName,
    ) -> impl Future<Output = Result<Named, StoreError>> + Send;

    fn disband(
        &self,
        org: TenantId,
        team: TeamId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    fn seat(
        &self,
        org: TenantId,
        team: TeamId,
        members: &[AccountId],
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    fn own(
        &self,
        org: TenantId,
        team: TeamId,
        projects: &[ProjectId],
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;
}

impl<T: Squads + Sync> Squads for &T {
    fn of(&self, org: TenantId) -> impl Future<Output = Result<Vec<Team>, StoreError>> + Send {
        (*self).of(org)
    }

    fn create(
        &self,
        org: TenantId,
        name: &TeamName,
    ) -> impl Future<Output = Result<Named, StoreError>> + Send {
        (*self).create(org, name)
    }

    fn rename(
        &self,
        org: TenantId,
        team: TeamId,
        name: &TeamName,
    ) -> impl Future<Output = Result<Named, StoreError>> + Send {
        (*self).rename(org, team, name)
    }

    fn disband(
        &self,
        org: TenantId,
        team: TeamId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        (*self).disband(org, team)
    }

    fn seat(
        &self,
        org: TenantId,
        team: TeamId,
        members: &[AccountId],
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        (*self).seat(org, team, members)
    }

    fn own(
        &self,
        org: TenantId,
        team: TeamId,
        projects: &[ProjectId],
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        (*self).own(org, team, projects)
    }
}

impl<T: Squads + Send + Sync> Squads for std::sync::Arc<T> {
    fn of(&self, org: TenantId) -> impl Future<Output = Result<Vec<Team>, StoreError>> + Send {
        self.as_ref().of(org)
    }

    fn create(
        &self,
        org: TenantId,
        name: &TeamName,
    ) -> impl Future<Output = Result<Named, StoreError>> + Send {
        self.as_ref().create(org, name)
    }

    fn rename(
        &self,
        org: TenantId,
        team: TeamId,
        name: &TeamName,
    ) -> impl Future<Output = Result<Named, StoreError>> + Send {
        self.as_ref().rename(org, team, name)
    }

    fn disband(
        &self,
        org: TenantId,
        team: TeamId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        self.as_ref().disband(org, team)
    }

    fn seat(
        &self,
        org: TenantId,
        team: TeamId,
        members: &[AccountId],
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        self.as_ref().seat(org, team, members)
    }

    fn own(
        &self,
        org: TenantId,
        team: TeamId,
        projects: &[ProjectId],
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        self.as_ref().own(org, team, projects)
    }
}
