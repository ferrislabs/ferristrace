use super::errors::ReadRefusal;
use super::key_ring_tests::{
    MINE, MemberOfMine, OURS, OneSession, Ring, THEIRS, a_project, an_org, scopes, token,
};
use super::readers::Readers;
use super::value_objects::{ApiSecret, KeyScope, KeyScopes, ReadGrant, Reader};
use crate::organisation::{OrgRole, Whoami};
use crate::secret::SessionToken;

fn readers(keys: &Ring) -> Readers<OneSession, MemberOfMine, &Ring> {
    Readers::new(Whoami::new(OneSession, MemberOfMine(OrgRole::Member)), keys)
}

#[tokio::test]
async fn a_member_session_reads_its_whole_organisation() {
    let keys = Ring::default();

    let grant = readers(&keys)
        .grant(&Reader::Session(token()), an_org(MINE))
        .await;

    assert_eq!(grant, Ok(ReadGrant::whole(an_org(MINE))));
}

#[tokio::test]
async fn a_session_reads_nothing_of_an_organisation_it_does_not_belong_to() {
    let keys = Ring::default();

    let session = readers(&keys)
        .grant(&Reader::Session(token()), an_org(THEIRS))
        .await;
    let unknown = readers(&keys)
        .grant(
            &Reader::Session(SessionToken::from_plaintext("ft_ss_nope")),
            an_org(MINE),
        )
        .await;

    assert_eq!(session, Err(ReadRefusal::NotYours));
    assert_eq!(unknown, Err(ReadRefusal::Unknown));
}

#[tokio::test]
async fn a_read_key_reads_through_its_grant() {
    let keys = Ring::default();
    let secret = keys.holding(
        an_org(MINE),
        Some(a_project(OURS)),
        scopes(&[KeyScope::Read]),
    );

    let grant = readers(&keys)
        .grant(&Reader::Key(secret), an_org(MINE))
        .await;

    assert_eq!(
        grant,
        Ok(ReadGrant {
            org: an_org(MINE),
            project: Some(a_project(OURS)),
        })
    );
}

#[tokio::test]
async fn a_read_key_asking_for_another_organisation_is_refused_like_a_stranger() {
    let keys = Ring::default();
    let secret = keys.holding(an_org(MINE), None, scopes(&[KeyScope::Read]));

    let grant = readers(&keys)
        .grant(&Reader::Key(secret), an_org(THEIRS))
        .await;

    assert_eq!(grant, Err(ReadRefusal::NotYours));
}

#[tokio::test]
async fn an_ingest_key_and_an_unknown_key_read_nothing() {
    let keys = Ring::default();
    let ingest = keys.holding(an_org(MINE), None, KeyScopes::ingest());

    let ingest = readers(&keys)
        .grant(&Reader::Key(ingest), an_org(MINE))
        .await;
    let unknown = readers(&keys)
        .grant(
            &Reader::Key(ApiSecret::from_plaintext("ft_live_000000000000.nope")),
            an_org(MINE),
        )
        .await;

    assert_eq!(ingest, Err(ReadRefusal::Unknown));
    assert_eq!(unknown, Err(ReadRefusal::Unknown));
}
