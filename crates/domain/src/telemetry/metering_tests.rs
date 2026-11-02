use std::sync::Mutex;
use uuid::Uuid;

use super::metering::Metering;
use super::ports::{UsageHistory, UsageReader};
use super::value_objects::{Consumed, DecompressedBytes, TenantId};
use crate::clock::UnixSeconds;
use crate::explore::{ExploreError, ReadError, Window};
use crate::identity::{AccountId, Agent, Email, Session, SessionError, SessionStore};
use crate::organisation::{Membership, Memberships, OrgRole, Whoami, WhoamiError};
use crate::secret::SessionToken;

const TOKEN: &str = "ct_ss_live";
const MINE: u128 = 1;
const THEIRS: u128 = 2;
const AN_HOUR: u64 = 3_600_000_000_000;

fn an_org(seed: u128) -> TenantId {
    TenantId::new(Uuid::from_u128(seed))
}

fn a_month() -> Window {
    Window::new(AN_HOUR, AN_HOUR * 800).expect("a window")
}

struct OneSession;

impl SessionStore for OneSession {
    async fn issue(
        &self,
        _account: AccountId,
        _org: Option<TenantId>,
        _from: Option<&Agent>,
    ) -> Result<SessionToken, SessionError> {
        Ok(SessionToken::from_plaintext(TOKEN))
    }

    async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
        if token.exposed() == TOKEN {
            Ok(Session {
                account: AccountId::new(Uuid::from_u128(9)),
                email: Email::try_from("person@example.com").expect("an address"),
                name: None,
                photo: None,
                org: None,
                expires_at: UnixSeconds::new(0),
            })
        } else {
            Err(SessionError::Unknown)
        }
    }

    async fn revoke(&self, _token: &SessionToken) -> Result<(), SessionError> {
        Ok(())
    }
}

struct MemberOfMine;

impl Memberships for MemberOfMine {
    async fn of(&self, _account: AccountId) -> Result<Vec<Membership>, WhoamiError> {
        Ok(vec![Membership {
            org: an_org(MINE),
            name: "Acme".into(),
            role: OrgRole::Member,
        }])
    }
}

struct Live(u64);

impl UsageReader for Live {
    async fn used(&self, _tenant: TenantId) -> DecompressedBytes {
        DecompressedBytes::new(self.0)
    }
}

#[derive(Debug, Default)]
struct Rolled {
    asked: Mutex<Vec<TenantId>>,
    included: Option<u64>,
}

impl UsageHistory for &Rolled {
    async fn hours(&self, org: TenantId, _window: Window) -> Result<Vec<Consumed>, ReadError> {
        self.asked.lock().expect("the lock").push(org);
        Ok(vec![
            Consumed {
                started_at_unix_nanos: AN_HOUR,
                ingested_bytes: 10,
            },
            Consumed {
                started_at_unix_nanos: AN_HOUR * 2,
                ingested_bytes: 20,
            },
        ])
    }

    async fn included(&self, _org: TenantId) -> Result<Option<DecompressedBytes>, ReadError> {
        Ok(self.included.map(DecompressedBytes::new))
    }
}

fn metering(live: u64, rolled: &Rolled) -> Metering<OneSession, MemberOfMine, Live, &Rolled> {
    Metering::new(Whoami::new(OneSession, MemberOfMine), Live(live), rolled)
}

fn token() -> SessionToken {
    SessionToken::from_plaintext(TOKEN)
}

#[tokio::test]
async fn the_month_to_date_total_comes_from_the_live_counter() {
    let rolled = Rolled {
        included: Some(1_000),
        ..Rolled::default()
    };

    let consumption = metering(999, &rolled)
        .consumption(&token(), an_org(MINE), a_month())
        .await
        .expect("a member may read");

    assert_eq!(consumption.ingested_bytes, 999);
    assert_eq!(
        consumption
            .hours
            .iter()
            .map(|h| h.ingested_bytes)
            .sum::<u64>(),
        30,
        "and the hours are the shape over time, which is a different number"
    );
    assert_eq!(consumption.included_bytes, Some(1_000));
}

#[tokio::test]
async fn a_plan_with_no_ceiling_includes_nothing_rather_than_zero() {
    let rolled = Rolled::default();

    let consumption = metering(999, &rolled)
        .consumption(&token(), an_org(MINE), a_month())
        .await
        .expect("a member may read");

    assert_eq!(
        consumption.included_bytes, None,
        "zero would read as a ceiling already passed"
    );
}

#[tokio::test]
async fn the_usage_of_another_organisation_is_refused_before_the_store_is_asked() {
    let rolled = Rolled::default();

    let refused = metering(999, &rolled)
        .consumption(&token(), an_org(THEIRS), a_month())
        .await;

    assert_eq!(refused.err(), Some(ExploreError::NotYours));
    assert!(rolled.asked.lock().expect("the lock").is_empty());
}

#[tokio::test]
async fn a_token_no_session_carries_reads_nothing() {
    let rolled = Rolled::default();

    let refused = metering(999, &rolled)
        .consumption(
            &SessionToken::from_plaintext("ct_ss_nope"),
            an_org(MINE),
            a_month(),
        )
        .await;

    assert_eq!(refused.err(), Some(ExploreError::NoSession));
}
