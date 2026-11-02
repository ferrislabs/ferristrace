use core::future::Future;

use super::entities::{Member, Membership, Organisation, Team};
use super::errors::WhoamiError;
use super::value_objects::{Admitted, Named, OrgName, OrgRole, TeamId, TeamName};
use crate::catalogue::ProjectId;
use crate::explore::ReadError;
use crate::identity::{AccountId, Email};
use crate::profile::Portrait;
use crate::secret::SecretHash;
use crate::telemetry::TenantId;

pub trait Orgs {
    fn only(
        &self,
        org: TenantId,
    ) -> impl Future<Output = Result<Option<Organisation>, ReadError>> + Send;

    fn rename(
        &self,
        org: TenantId,
        name: &OrgName,
    ) -> impl Future<Output = Result<(), ReadError>> + Send;

    fn set_logo(
        &self,
        org: TenantId,
        logo: Option<&Portrait>,
    ) -> impl Future<Output = Result<(), ReadError>> + Send;
}

impl<T: Orgs + Sync> Orgs for &T {
    fn only(
        &self,
        org: TenantId,
    ) -> impl Future<Output = Result<Option<Organisation>, ReadError>> + Send {
        (*self).only(org)
    }

    fn rename(
        &self,
        org: TenantId,
        name: &OrgName,
    ) -> impl Future<Output = Result<(), ReadError>> + Send {
        (*self).rename(org, name)
    }

    fn set_logo(
        &self,
        org: TenantId,
        logo: Option<&Portrait>,
    ) -> impl Future<Output = Result<(), ReadError>> + Send {
        (*self).set_logo(org, logo)
    }
}

impl<T: Orgs + Send + Sync> Orgs for std::sync::Arc<T> {
    fn only(
        &self,
        org: TenantId,
    ) -> impl Future<Output = Result<Option<Organisation>, ReadError>> + Send {
        self.as_ref().only(org)
    }

    fn rename(
        &self,
        org: TenantId,
        name: &OrgName,
    ) -> impl Future<Output = Result<(), ReadError>> + Send {
        self.as_ref().rename(org, name)
    }

    fn set_logo(
        &self,
        org: TenantId,
        logo: Option<&Portrait>,
    ) -> impl Future<Output = Result<(), ReadError>> + Send {
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
    fn of(&self, org: TenantId) -> impl Future<Output = Result<Vec<Member>, ReadError>> + Send;

    fn grant(
        &self,
        org: TenantId,
        account: AccountId,
        role: OrgRole,
    ) -> impl Future<Output = Result<Option<Member>, ReadError>> + Send;

    fn remove(
        &self,
        org: TenantId,
        account: AccountId,
    ) -> impl Future<Output = Result<bool, ReadError>> + Send;

    fn admit(
        &self,
        org: TenantId,
        email: &Email,
        role: OrgRole,
        way_in: SecretHash,
    ) -> impl Future<Output = Result<Admitted, ReadError>> + Send;
}

impl<T: Roll + Sync> Roll for &T {
    fn of(&self, org: TenantId) -> impl Future<Output = Result<Vec<Member>, ReadError>> + Send {
        (*self).of(org)
    }

    fn grant(
        &self,
        org: TenantId,
        account: AccountId,
        role: OrgRole,
    ) -> impl Future<Output = Result<Option<Member>, ReadError>> + Send {
        (*self).grant(org, account, role)
    }

    fn remove(
        &self,
        org: TenantId,
        account: AccountId,
    ) -> impl Future<Output = Result<bool, ReadError>> + Send {
        (*self).remove(org, account)
    }

    fn admit(
        &self,
        org: TenantId,
        email: &Email,
        role: OrgRole,
        way_in: SecretHash,
    ) -> impl Future<Output = Result<Admitted, ReadError>> + Send {
        (*self).admit(org, email, role, way_in)
    }
}

impl<T: Roll + Send + Sync> Roll for std::sync::Arc<T> {
    fn of(&self, org: TenantId) -> impl Future<Output = Result<Vec<Member>, ReadError>> + Send {
        self.as_ref().of(org)
    }

    fn grant(
        &self,
        org: TenantId,
        account: AccountId,
        role: OrgRole,
    ) -> impl Future<Output = Result<Option<Member>, ReadError>> + Send {
        self.as_ref().grant(org, account, role)
    }

    fn remove(
        &self,
        org: TenantId,
        account: AccountId,
    ) -> impl Future<Output = Result<bool, ReadError>> + Send {
        self.as_ref().remove(org, account)
    }

    fn admit(
        &self,
        org: TenantId,
        email: &Email,
        role: OrgRole,
        way_in: SecretHash,
    ) -> impl Future<Output = Result<Admitted, ReadError>> + Send {
        self.as_ref().admit(org, email, role, way_in)
    }
}

pub trait Squads {
    fn of(&self, org: TenantId) -> impl Future<Output = Result<Vec<Team>, ReadError>> + Send;

    fn create(
        &self,
        org: TenantId,
        name: &TeamName,
    ) -> impl Future<Output = Result<Named, ReadError>> + Send;

    fn rename(
        &self,
        org: TenantId,
        team: TeamId,
        name: &TeamName,
    ) -> impl Future<Output = Result<Named, ReadError>> + Send;

    fn disband(
        &self,
        org: TenantId,
        team: TeamId,
    ) -> impl Future<Output = Result<bool, ReadError>> + Send;

    fn seat(
        &self,
        org: TenantId,
        team: TeamId,
        members: &[AccountId],
    ) -> impl Future<Output = Result<bool, ReadError>> + Send;

    fn own(
        &self,
        org: TenantId,
        team: TeamId,
        projects: &[ProjectId],
    ) -> impl Future<Output = Result<bool, ReadError>> + Send;
}

impl<T: Squads + Sync> Squads for &T {
    fn of(&self, org: TenantId) -> impl Future<Output = Result<Vec<Team>, ReadError>> + Send {
        (*self).of(org)
    }

    fn create(
        &self,
        org: TenantId,
        name: &TeamName,
    ) -> impl Future<Output = Result<Named, ReadError>> + Send {
        (*self).create(org, name)
    }

    fn rename(
        &self,
        org: TenantId,
        team: TeamId,
        name: &TeamName,
    ) -> impl Future<Output = Result<Named, ReadError>> + Send {
        (*self).rename(org, team, name)
    }

    fn disband(
        &self,
        org: TenantId,
        team: TeamId,
    ) -> impl Future<Output = Result<bool, ReadError>> + Send {
        (*self).disband(org, team)
    }

    fn seat(
        &self,
        org: TenantId,
        team: TeamId,
        members: &[AccountId],
    ) -> impl Future<Output = Result<bool, ReadError>> + Send {
        (*self).seat(org, team, members)
    }

    fn own(
        &self,
        org: TenantId,
        team: TeamId,
        projects: &[ProjectId],
    ) -> impl Future<Output = Result<bool, ReadError>> + Send {
        (*self).own(org, team, projects)
    }
}

impl<T: Squads + Send + Sync> Squads for std::sync::Arc<T> {
    fn of(&self, org: TenantId) -> impl Future<Output = Result<Vec<Team>, ReadError>> + Send {
        self.as_ref().of(org)
    }

    fn create(
        &self,
        org: TenantId,
        name: &TeamName,
    ) -> impl Future<Output = Result<Named, ReadError>> + Send {
        self.as_ref().create(org, name)
    }

    fn rename(
        &self,
        org: TenantId,
        team: TeamId,
        name: &TeamName,
    ) -> impl Future<Output = Result<Named, ReadError>> + Send {
        self.as_ref().rename(org, team, name)
    }

    fn disband(
        &self,
        org: TenantId,
        team: TeamId,
    ) -> impl Future<Output = Result<bool, ReadError>> + Send {
        self.as_ref().disband(org, team)
    }

    fn seat(
        &self,
        org: TenantId,
        team: TeamId,
        members: &[AccountId],
    ) -> impl Future<Output = Result<bool, ReadError>> + Send {
        self.as_ref().seat(org, team, members)
    }

    fn own(
        &self,
        org: TenantId,
        team: TeamId,
        projects: &[ProjectId],
    ) -> impl Future<Output = Result<bool, ReadError>> + Send {
        self.as_ref().own(org, team, projects)
    }
}
