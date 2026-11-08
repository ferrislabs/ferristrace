use super::org_clock::OrgClock;
use super::ports::Orgs;
use super::value_objects::{OrgRole, OrgTimezone};
use super::{Organisation, Whoami};
use crate::access::key_ring_tests::{
    MINE, MemberOfMine, OneSession, Ring, THEIRS, an_org, scopes, token,
};
use crate::access::{KeyScope, ReadRefusal, Reader, Readers};
use crate::agents::registry_tests::Book;
use crate::agents::{AgentCall, AgentScope, VerifiedToken};
use crate::store::StoreError;
use crate::telemetry::TenantId;

struct InParis;

impl Orgs for InParis {
    async fn only(&self, org: TenantId) -> Result<Option<Organisation>, StoreError> {
        Ok((org == an_org(MINE)).then(|| Organisation {
            id: org,
            name: "Acme".into(),
            plan: "Self-hosted".into(),
            created_at_unix_nanos: 0,
            logo: None,
            retention: None,
            retention_cap: None,
            timezone: OrgTimezone::try_from("Europe/Paris").expect("an IANA name"),
        }))
    }

    async fn rename(&self, _: TenantId, _: &super::OrgName) -> Result<(), StoreError> {
        Ok(())
    }

    async fn disband(&self, _: TenantId) -> Result<bool, StoreError> {
        Ok(true)
    }

    async fn set_logo(
        &self,
        _: TenantId,
        _: Option<&crate::profile::Portrait>,
    ) -> Result<(), StoreError> {
        Ok(())
    }

    async fn set_retention(
        &self,
        _: TenantId,
        _: Option<crate::telemetry::RetentionDays>,
    ) -> Result<(), StoreError> {
        Ok(())
    }

    async fn set_timezone(&self, _: TenantId, _: &OrgTimezone) -> Result<(), StoreError> {
        Ok(())
    }
}

fn agent_calling(client: &str, claimed: &[&str]) -> Reader {
    Reader::Agent(AgentCall {
        token: VerifiedToken {
            client_id: client.into(),
            scopes: claimed.iter().map(|scope| (*scope).into()).collect(),
            jti: "jti-1".into(),
            expires_at_unix_secs: 4_000_000_000,
            delegated: None,
        },
        route: "/api/orgs/{org}/clock".into(),
    })
}

#[tokio::test]
async fn a_session_reads_the_zone_of_its_organisation() {
    let keys = Ring::default();
    let clock = OrgClock::new(
        Readers::new(
            Whoami::new(OneSession, MemberOfMine(OrgRole::Member)),
            &keys,
        ),
        InParis,
    );

    let zone = clock
        .timezone(&Reader::Session(token()), an_org(MINE))
        .await;

    assert_eq!(
        zone.map(|zone| zone.to_string()),
        Ok("Europe/Paris".to_owned())
    );
}

#[tokio::test]
async fn a_session_of_another_organisation_reads_nothing() {
    let keys = Ring::default();
    let clock = OrgClock::new(
        Readers::new(
            Whoami::new(OneSession, MemberOfMine(OrgRole::Member)),
            &keys,
        ),
        InParis,
    );

    let zone = clock
        .timezone(&Reader::Session(token()), an_org(THEIRS))
        .await;

    assert_eq!(zone, Err(ReadRefusal::NotYours));
}

#[tokio::test]
async fn a_read_key_reads_the_zone() {
    let keys = Ring::default();
    let secret = keys.holding(an_org(MINE), None, scopes(&[KeyScope::Read]));
    let clock = OrgClock::new(
        Readers::new(
            Whoami::new(OneSession, MemberOfMine(OrgRole::Member)),
            &keys,
        ),
        InParis,
    );

    let zone = clock.timezone(&Reader::Key(secret), an_org(MINE)).await;

    assert_eq!(
        zone.map(|zone| zone.to_string()),
        Ok("Europe/Paris".to_owned())
    );
}

#[tokio::test]
async fn an_agent_needs_telemetry_read_and_its_read_is_audited() {
    let book = Book::default();
    book.enrol(an_org(MINE), "bot", &[AgentScope::TelemetryRead], None);
    book.enrol(
        an_org(MINE),
        "writer",
        &[AgentScope::IncidentsComment],
        None,
    );
    let clock = OrgClock::new(
        Readers::new(
            Whoami::new(OneSession, MemberOfMine(OrgRole::Member)),
            &book,
        ),
        InParis,
    );

    let reads = clock
        .timezone(&agent_calling("bot", &["telemetry:read"]), an_org(MINE))
        .await;
    let out_of_scope = clock
        .timezone(
            &agent_calling("writer", &["incidents:comment"]),
            an_org(MINE),
        )
        .await;

    assert_eq!(
        reads.map(|zone| zone.to_string()),
        Ok("Europe/Paris".to_owned())
    );
    assert_eq!(out_of_scope, Err(ReadRefusal::OutOfScope));
    assert_eq!(book.uses().len(), 1);
}
