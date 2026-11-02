use uuid::Uuid;

use super::entities::Membership;
use super::value_objects::{OrgRole, Who};
use crate::identity::{AccountId, Email};
use crate::telemetry::TenantId;

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
