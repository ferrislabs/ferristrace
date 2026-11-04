use super::errors::ReadRefusal;
use super::key_ring_tests::{
    MINE, MemberOfMine, OURS, OneSession, Ring, THEIRS, a_project, an_org, scopes, token,
};
use super::readers::Readers;
use super::value_objects::{ApiSecret, KeyScope, KeyScopes, ReadGrant, Reader};
use crate::agents::registry_tests::Book;
use crate::agents::{AgentCall, AgentScope, VerifiedToken};
use crate::audit::{Action, Actor, Target};
use crate::organisation::{OrgRole, Whoami};
use crate::secret::SessionToken;

fn agent_readers(book: &Book) -> Readers<OneSession, MemberOfMine, &Book> {
    Readers::new(Whoami::new(OneSession, MemberOfMine(OrgRole::Member)), book)
}

fn calling(client_id: &str, scopes: &[&str]) -> Reader {
    Reader::Agent(AgentCall {
        token: VerifiedToken {
            client_id: client_id.into(),
            scopes: scopes.iter().map(|scope| (*scope).into()).collect(),
            jti: "jti-1".into(),
            expires_at_unix_secs: 4_000_000_000,
            delegated: None,
        },
        route: "/api/orgs/{org}/traces".into(),
    })
}

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
            &Reader::Session(SessionToken::for_subject("ft_ss_nope")),
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

#[tokio::test]
async fn an_agent_reads_through_its_scopes_and_the_read_is_audited_with_the_jti() {
    let book = Book::default();
    let id = book.enrol(an_org(MINE), "bot", &[AgentScope::TelemetryRead], None);

    let grant = agent_readers(&book)
        .grant(&calling("bot", &["telemetry:read"]), an_org(MINE))
        .await;

    assert_eq!(grant, Ok(ReadGrant::whole(an_org(MINE))));
    let uses = book.uses();
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].0, id);
    assert_eq!(uses[0].1.actor, Actor::Agent(id));
    assert_eq!(uses[0].1.action, Action::AgentRead);
    assert_eq!(uses[0].1.target, Target::org(an_org(MINE)));
    assert_eq!(uses[0].1.detail.get("jti"), Some(Some("jti-1")));
    assert_eq!(
        uses[0].1.detail.get("route"),
        Some(Some("/api/orgs/{org}/traces"))
    );
}

#[tokio::test]
async fn every_read_of_an_agent_writes_its_own_entry() {
    let book = Book::default();
    book.enrol(an_org(MINE), "bot", &[AgentScope::TelemetryRead], None);
    let readers = agent_readers(&book);
    let reader = calling("bot", &["telemetry:read"]);

    readers.grant(&reader, an_org(MINE)).await.expect("a read");
    readers.grant(&reader, an_org(MINE)).await.expect("a read");

    assert_eq!(book.uses().len(), 2);
}

#[tokio::test]
async fn an_agent_bound_to_a_project_reads_only_that_project() {
    let book = Book::default();
    book.enrol(
        an_org(MINE),
        "bot",
        &[AgentScope::TelemetryRead],
        Some(a_project(OURS)),
    );

    let grant = agent_readers(&book)
        .grant(&calling("bot", &["telemetry:read"]), an_org(MINE))
        .await
        .expect("a grant");

    assert_eq!(grant.project, Some(a_project(OURS)));
    assert_eq!(grant.narrow(None), Ok(Some(a_project(OURS))));
    assert_eq!(
        grant.narrow(Some(a_project(OURS))),
        Ok(Some(a_project(OURS)))
    );
    assert_eq!(
        grant.narrow(Some(a_project(40))),
        Err(ReadRefusal::NotYours)
    );
}

#[tokio::test]
async fn a_token_claiming_more_than_the_agent_was_granted_gains_nothing() {
    let book = Book::default();
    book.enrol(an_org(MINE), "bot", &[AgentScope::IncidentsComment], None);

    let refused = agent_readers(&book)
        .grant(
            &calling("bot", &["telemetry:read", "incidents:comment"]),
            an_org(MINE),
        )
        .await;

    assert_eq!(refused, Err(ReadRefusal::OutOfScope));
    assert!(book.uses().is_empty());
}

#[tokio::test]
async fn an_agent_granted_a_scope_the_token_does_not_claim_gains_nothing() {
    let book = Book::default();
    book.enrol(an_org(MINE), "bot", &[AgentScope::TelemetryRead], None);

    for claimed in [&[][..], &["openid"][..], &["incidents:comment"][..]] {
        let refused = agent_readers(&book)
            .grant(&calling("bot", claimed), an_org(MINE))
            .await;

        assert_eq!(refused, Err(ReadRefusal::OutOfScope), "{claimed:?}");
    }
    assert!(book.uses().is_empty());
}

#[tokio::test]
async fn an_agent_of_another_organisation_reads_nothing_here() {
    let book = Book::default();
    book.enrol(an_org(THEIRS), "bot", &[AgentScope::TelemetryRead], None);

    let refused = agent_readers(&book)
        .grant(&calling("bot", &["telemetry:read"]), an_org(MINE))
        .await;

    assert_eq!(refused, Err(ReadRefusal::Unknown));
    assert!(book.uses().is_empty());
}

#[tokio::test]
async fn an_unregistered_client_reads_nothing() {
    let book = Book::default();
    book.enrol(an_org(MINE), "bot", &[AgentScope::TelemetryRead], None);

    let refused = agent_readers(&book)
        .grant(&calling("impostor", &["telemetry:read"]), an_org(MINE))
        .await;

    assert_eq!(refused, Err(ReadRefusal::Unknown));
    assert!(book.uses().is_empty());
}

#[tokio::test]
async fn a_revoked_agent_is_refused_whatever_its_token_says() {
    let book = Book::default();
    let id = book.enrol(an_org(MINE), "bot", &[AgentScope::TelemetryRead], None);
    let reader = calling("bot", &["telemetry:read"]);
    agent_readers(&book)
        .grant(&reader, an_org(MINE))
        .await
        .expect("a read before the revocation");
    book.revoke_now(id);

    let refused = agent_readers(&book).grant(&reader, an_org(MINE)).await;

    assert_eq!(refused, Err(ReadRefusal::Unknown));
    assert_eq!(book.uses().len(), 1);
}

#[tokio::test]
async fn a_client_registered_again_after_a_revocation_reads_through_the_new_agent() {
    let book = Book::default();
    let old = book.enrol(an_org(MINE), "bot", &[AgentScope::TelemetryRead], None);
    book.revoke_now(old);
    let new = book.enrol(an_org(MINE), "bot", &[AgentScope::TelemetryRead], None);

    agent_readers(&book)
        .grant(&calling("bot", &["telemetry:read"]), an_org(MINE))
        .await
        .expect("the live agent reads");

    assert_eq!(book.uses()[0].0, new);
}

#[tokio::test]
async fn an_agent_reports_its_own_grant_with_the_effective_scopes() {
    let book = Book::default();
    let id = book.enrol(
        an_org(MINE),
        "bot",
        &[AgentScope::TelemetryRead, AgentScope::IncidentsComment],
        Some(a_project(OURS)),
    );
    let Reader::Agent(call) = calling("bot", &["telemetry:read", "incidents:resolve"]) else {
        unreachable!()
    };

    let grant = agent_readers(&book)
        .agent(&call, an_org(MINE), None)
        .await
        .expect("the agent is known");
    let nothing = agent_readers(&book)
        .agent(
            &AgentCall {
                token: VerifiedToken {
                    scopes: vec!["openid".into()],
                    ..call.token.clone()
                },
                route: call.route.clone(),
            },
            an_org(MINE),
            None,
        )
        .await;

    assert_eq!(grant.agent, id);
    assert_eq!(grant.scopes, [AgentScope::TelemetryRead]);
    assert_eq!(grant.project, Some(a_project(OURS)));
    assert_eq!(&*grant.jti, "jti-1");
    assert_eq!(nothing, Err(ReadRefusal::OutOfScope));
}

#[tokio::test]
async fn unregistered_and_revoked_clients_are_refused_identically() {
    let book = Book::default();
    let id = book.enrol(
        an_org(MINE),
        "revoked-bot",
        &[AgentScope::TelemetryRead],
        None,
    );
    book.revoke_now(id);
    let readers = agent_readers(&book);

    let revoked = readers
        .grant(&calling("revoked-bot", &["telemetry:read"]), an_org(MINE))
        .await;
    let unregistered = readers
        .grant(
            &calling("never-registered", &["telemetry:read"]),
            an_org(MINE),
        )
        .await;

    assert_eq!(revoked, unregistered);
    assert_eq!(revoked, Err(ReadRefusal::Unknown));
}

#[tokio::test]
async fn spec_l7_4_a_delegated_read_names_the_person_in_its_audit_entry() {
    use crate::agents::Delegation;

    let book = Book::default();
    book.enrol(an_org(MINE), "bot", &[AgentScope::TelemetryRead], None);
    let Reader::Agent(mut call) = calling("bot", &["telemetry:read"]) else {
        unreachable!()
    };
    call.token.delegated = Some(Delegation {
        subject: "sub-dana".into(),
        email: Some("dana@example.com".into()),
        act_sub: None,
        act_client_id: Some("mcp-gateway".into()),
    });

    agent_readers(&book)
        .grant(&Reader::Agent(call), an_org(MINE))
        .await
        .expect("a delegated read");

    let uses = book.uses();
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].1.detail.get("on_behalf_of"), Some(Some("sub-dana")));
    assert_eq!(
        uses[0].1.detail.get("on_behalf_of_email"),
        Some(Some("dana@example.com"))
    );
    assert_eq!(uses[0].1.detail.get("jti"), Some(Some("jti-1")));
}

#[tokio::test]
async fn an_agent_acting_for_nobody_writes_no_on_behalf_of() {
    let book = Book::default();
    book.enrol(an_org(MINE), "bot", &[AgentScope::TelemetryRead], None);

    agent_readers(&book)
        .grant(&calling("bot", &["telemetry:read"]), an_org(MINE))
        .await
        .expect("a read");

    assert_eq!(book.uses()[0].1.detail.get("on_behalf_of"), None);
}
