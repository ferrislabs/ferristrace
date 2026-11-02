use uuid::Uuid;

use super::errors::LinkRefused;
use super::link::{LinkClaim, LinkKey, Purpose};
use super::value_objects::{StatusPageId, SubscriberId};
use crate::clock::UnixSeconds;

fn claim(purpose: Purpose) -> LinkClaim {
    LinkClaim {
        purpose,
        page: StatusPageId::new(Uuid::from_u128(1)),
        subscriber: SubscriberId::new(Uuid::from_u128(2)),
    }
}

fn key() -> LinkKey {
    LinkKey::derive(&[7; 32])
}

#[test]
fn a_signed_link_opens_back_to_its_claim_before_it_expires() {
    for purpose in [Purpose::Confirm, Purpose::Unsubscribe] {
        let token = key().sign(&claim(purpose), UnixSeconds::new(100));

        assert_eq!(
            key().open(&token, purpose, UnixSeconds::new(99)),
            Ok(claim(purpose))
        );
        assert_eq!(
            key().open(&token, purpose, UnixSeconds::new(100)),
            Err(LinkRefused::Expired)
        );
    }
}

#[test]
fn a_link_for_one_purpose_does_not_serve_the_other() {
    let token = key().sign(&claim(Purpose::Confirm), UnixSeconds::new(100));

    assert_eq!(
        key().open(&token, Purpose::Unsubscribe, UnixSeconds::new(1)),
        Err(LinkRefused::Tampered)
    );
}

#[test]
fn a_changed_or_foreign_link_is_refused() {
    let token = key().sign(&claim(Purpose::Unsubscribe), UnixSeconds::new(100));
    let lengthened = token.replacen(".100.", ".999.", 1);
    let theirs = LinkKey::derive(&[8; 32]);

    assert_eq!(
        key().open(&lengthened, Purpose::Unsubscribe, UnixSeconds::new(1)),
        Err(LinkRefused::Tampered)
    );
    assert_eq!(
        theirs.open(&token, Purpose::Unsubscribe, UnixSeconds::new(1)),
        Err(LinkRefused::Tampered)
    );
    for junk in ["", "v1", "v1.u.x.y.1.zz", "nope.nope"] {
        assert_eq!(
            key().open(junk, Purpose::Unsubscribe, UnixSeconds::new(1)),
            Err(LinkRefused::Tampered),
            "{junk}"
        );
    }
}

#[test]
fn a_link_key_differs_from_the_page_acknowledgement_key() {
    let ack = crate::oncall::AckKey::derive(&[7; 32]);
    let token = key().sign(&claim(Purpose::Confirm), UnixSeconds::new(100));

    assert!(ack.open(&token, UnixSeconds::new(1)).is_err());
    assert_eq!(format!("{:?}", key()), "LinkKey(redacted)");
}
