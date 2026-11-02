use uuid::Uuid;

use super::ack::{AckClaim, AckKey};
use super::errors::AckRefused;
use super::value_objects::PageId;
use crate::clock::UnixSeconds;
use crate::identity::AccountId;
use crate::telemetry::TenantId;

fn claim() -> AckClaim {
    AckClaim {
        org: TenantId::new(Uuid::from_u128(1)),
        page: PageId::new(Uuid::from_u128(2)),
        account: AccountId::new(Uuid::from_u128(3)),
    }
}

fn key() -> AckKey {
    AckKey::derive(&[7_u8; 32])
}

#[test]
fn a_signed_link_opens_to_what_it_was_signed_for() {
    let token = key().sign(&claim(), UnixSeconds::new(1_000));

    assert_eq!(key().open(&token, UnixSeconds::new(999)), Ok(claim()));
}

#[test]
fn a_link_expires_at_its_deadline() {
    let token = key().sign(&claim(), UnixSeconds::new(1_000));

    assert_eq!(
        key().open(&token, UnixSeconds::new(1_000)),
        Err(AckRefused::Expired)
    );
}

#[test]
fn a_link_changed_anywhere_is_refused() {
    let token = key().sign(&claim(), UnixSeconds::new(1_000)).to_string();
    let other_page = token.replace(
        &Uuid::from_u128(2).simple().to_string(),
        &Uuid::from_u128(4).simple().to_string(),
    );
    let later = token.replace(".1000.", ".9000.");
    let mut flipped = token.clone().into_bytes();
    let last = flipped.len() - 1;
    flipped[last] = if flipped[last] == b'0' { b'1' } else { b'0' };
    let flipped = String::from_utf8(flipped).expect("text");

    for tampered in [
        other_page,
        later,
        flipped,
        "nonsense".to_owned(),
        String::new(),
    ] {
        assert_eq!(
            key().open(&tampered, UnixSeconds::new(10)),
            Err(AckRefused::Tampered),
            "{tampered}"
        );
    }
}

#[test]
fn another_deployment_cannot_forge_a_link() {
    let theirs = AckKey::derive(&[8_u8; 32]).sign(&claim(), UnixSeconds::new(1_000));

    assert_eq!(
        key().open(&theirs, UnixSeconds::new(10)),
        Err(AckRefused::Tampered)
    );
}

#[test]
fn an_expired_forgery_is_still_a_forgery() {
    let theirs = AckKey::derive(&[8_u8; 32]).sign(&claim(), UnixSeconds::new(1));

    assert_eq!(
        key().open(&theirs, UnixSeconds::new(10)),
        Err(AckRefused::Tampered)
    );
}

#[test]
fn a_key_never_renders() {
    assert_eq!(format!("{:?}", key()), "AckKey(redacted)");
}
