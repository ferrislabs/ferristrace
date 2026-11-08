use uuid::Uuid;

use super::errors::{BadKeyName, BadScopes, ReadRefusal};
use super::value_objects::{
    ApiSecret, KeyName, KeyScope, KeyScopes, KeyTtl, MAX_KEY_NAME_BYTES, ReadGrant,
};
use crate::catalogue::ProjectId;
use crate::telemetry::TenantId;

fn a_project(seed: u128) -> ProjectId {
    ProjectId::new(Uuid::from_u128(seed))
}

fn an_org() -> TenantId {
    TenantId::new(Uuid::from_u128(1))
}

#[test]
fn a_scope_reads_back_as_it_is_spelled() {
    for scope in [KeyScope::Ingest, KeyScope::Read] {
        assert_eq!(KeyScope::parse(scope.as_str()), Some(scope));
    }
    assert_eq!(KeyScope::parse("write"), None);
    assert_eq!(KeyScope::parse("Ingest"), None);
}

#[test]
fn a_key_carries_at_least_one_scope() {
    assert_eq!(KeyScopes::new(&[]), Err(BadScopes::Empty));
}

#[test]
fn a_key_carries_each_scope_once() {
    assert_eq!(
        KeyScopes::new(&[KeyScope::Read, KeyScope::Read]),
        Err(BadScopes::Repeated)
    );
}

#[test]
fn scopes_hold_what_they_were_given_in_one_order() {
    let both = KeyScopes::new(&[KeyScope::Read, KeyScope::Ingest]).expect("two scopes");

    assert!(both.contains(KeyScope::Ingest));
    assert!(both.contains(KeyScope::Read));
    assert_eq!(both.as_slice(), &[KeyScope::Ingest, KeyScope::Read]);
    assert_eq!(
        both,
        KeyScopes::new(&[KeyScope::Ingest, KeyScope::Read]).expect("the same two")
    );
}

#[test]
fn scopes_parse_from_their_spellings_and_refuse_an_unknown_one() {
    assert_eq!(
        KeyScopes::parse(&["read"]),
        KeyScopes::new(&[KeyScope::Read])
    );
    assert_eq!(
        KeyScopes::parse(&["read", "admin"]),
        Err(BadScopes::Unknown)
    );
    assert_eq!(KeyScopes::parse::<&str>(&[]), Err(BadScopes::Empty));
}

#[test]
fn the_ingest_scopes_carry_ingest_alone() {
    assert_eq!(KeyScopes::ingest().as_slice(), &[KeyScope::Ingest]);
}

#[test]
fn a_key_name_is_kept_without_the_space_around_it() {
    assert_eq!(
        KeyName::try_from("  edge collector \n")
            .expect("a name")
            .as_str(),
        "edge collector"
    );
}

#[test]
fn a_blank_key_name_is_refused() {
    assert_eq!(KeyName::try_from(""), Err(BadKeyName::Blank));
    assert_eq!(KeyName::try_from(" \t "), Err(BadKeyName::Blank));
}

#[test]
fn a_key_name_past_the_ceiling_is_refused() {
    let long = "a".repeat(MAX_KEY_NAME_BYTES + 1);

    assert_eq!(
        KeyName::try_from(long.as_str()),
        Err(BadKeyName::TooLong(MAX_KEY_NAME_BYTES))
    );
    assert!(KeyName::try_from("a".repeat(MAX_KEY_NAME_BYTES).as_str()).is_ok());
}

#[test]
fn a_secret_never_renders_itself() {
    let secret = ApiSecret::from_plaintext("ft_live_abcdefabcdef.the-secret");

    assert_eq!(format!("{secret:?}"), "ApiSecret(redacted)");
}

#[test]
fn a_secret_splits_like_an_ingest_key() {
    let secret = ApiSecret::from_plaintext("ft_live_abcdefabcdef.the-secret");

    assert_eq!(secret.split(), Some(("ft_live_abcdefabcdef", "the-secret")));
    assert_eq!(ApiSecret::from_plaintext("ft_live_abc.").split(), None);
}

#[test]
fn only_a_live_prefixed_secret_looks_like_a_key() {
    assert!(ApiSecret::from_plaintext("ft_live_abcdefabcdef.the-secret").looks_like_one());
    assert!(!ApiSecret::from_plaintext("ft_ss_abcdef.the-secret").looks_like_one());
    assert!(!ApiSecret::from_plaintext("ft_live_abcdef").looks_like_one());
}

#[test]
fn a_whole_organisation_grant_reads_whichever_project_is_asked() {
    let grant = ReadGrant::whole(an_org());

    assert_eq!(grant.narrow(None), Ok(None));
    assert_eq!(grant.narrow(Some(a_project(7))), Ok(Some(a_project(7))));
}

#[test]
fn a_project_bound_grant_narrows_an_open_question_to_its_project() {
    let grant = ReadGrant {
        org: an_org(),
        project: Some(a_project(7)),
    };

    assert_eq!(grant.narrow(None), Ok(Some(a_project(7))));
    assert_eq!(grant.narrow(Some(a_project(7))), Ok(Some(a_project(7))));
}

#[test]
fn a_project_bound_grant_refuses_another_project_like_a_stranger() {
    let grant = ReadGrant {
        org: an_org(),
        project: Some(a_project(7)),
    };

    assert_eq!(grant.narrow(Some(a_project(8))), Err(ReadRefusal::NotYours));
}

#[test]
fn a_ttl_reads_back_from_its_spelling() {
    for ttl in [
        KeyTtl::Days30,
        KeyTtl::Days90,
        KeyTtl::Days365,
        KeyTtl::Never,
    ] {
        assert_eq!(KeyTtl::parse(ttl.as_str()), Some(ttl));
    }
    assert_eq!(KeyTtl::parse("30 days"), None);
}

#[test]
fn never_expires_whatever_it_was_issued_at() {
    assert_eq!(KeyTtl::Never.expires_at(0), None);
    assert_eq!(KeyTtl::Never.expires_at(u64::MAX), None);
}

#[test]
fn a_ttl_lands_exactly_that_many_days_after_issue() {
    const NANOS_PER_DAY: u64 = 86_400 * 1_000_000_000;
    let issued_at = 1_000;

    assert_eq!(
        KeyTtl::Days30.expires_at(issued_at),
        Some(issued_at + 30 * NANOS_PER_DAY)
    );
    assert_eq!(
        KeyTtl::Days90.expires_at(issued_at),
        Some(issued_at + 90 * NANOS_PER_DAY)
    );
    assert_eq!(
        KeyTtl::Days365.expires_at(issued_at),
        Some(issued_at + 365 * NANOS_PER_DAY)
    );
}

#[test]
fn a_ttl_saturates_rather_than_overflows_at_the_edge_of_time() {
    assert_eq!(KeyTtl::Days365.expires_at(u64::MAX), Some(u64::MAX));
}
