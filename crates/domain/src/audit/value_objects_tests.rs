use uuid::Uuid;

use super::value_objects::{
    Action, Actor, Cursor, DEFAULT_PAGE, Detail, EntryId, MAX_PAGE, PageSize, Target, TargetKind,
};
use crate::access::KeyId;
use crate::identity::AccountId;
use crate::telemetry::TenantId;

#[test]
fn every_action_reads_back_from_its_spelling() {
    for action in Action::ALL {
        assert_eq!(Action::parse(action.as_str()), Some(action));
    }
}

#[test]
fn the_spellings_are_stable_and_dotted() {
    let spelled: Vec<&str> = Action::ALL.iter().map(|action| action.as_str()).collect();

    assert_eq!(
        spelled,
        vec![
            "org.founded",
            "org.renamed",
            "org.logo_set",
            "org.logo_removed",
            "org.retention_set",
            "project.created",
            "project.renamed",
            "project.deleted",
            "project.assigned",
            "team.created",
            "team.renamed",
            "team.disbanded",
            "team.seated",
            "team.projects_owned",
            "member.invited",
            "member.role_granted",
            "member.removed",
            "key.issued",
            "key.revoked",
            "issue.decided",
            "issue.assigned",
            "account.created_by_operator",
            "reset_link.issued_by_operator",
        ]
    );
}

#[test]
fn an_unknown_action_is_refused() {
    assert_eq!(Action::parse("org.deleted"), None);
}

#[test]
fn an_actor_reads_back_from_its_kind_and_id() {
    let account = Actor::Account(AccountId::new(Uuid::from_u128(4)));
    let key = Actor::ApiKey(KeyId::new(Uuid::from_u128(5)));

    for actor in [account, key, Actor::Operator] {
        assert_eq!(Actor::of(actor.kind(), actor.id()), Some(actor));
    }
}

#[test]
fn the_operator_carries_no_id_and_an_account_always_does() {
    assert_eq!(Actor::Operator.id(), None);
    assert_eq!(Actor::of("account", None), None);
    assert_eq!(Actor::of("operator", Some(Uuid::from_u128(1))), None);
    assert_eq!(Actor::of("robot", None), None);
}

#[test]
fn a_target_names_its_kind_and_the_id_it_was_built_from() {
    let org = TenantId::new(Uuid::from_u128(1));

    let target = Target::org(org);

    assert_eq!(target.kind, TargetKind::Org);
    assert_eq!(target.id.as_deref(), Some(org.to_string().as_str()));
    assert_eq!(
        TargetKind::parse(target.kind.as_str()),
        Some(TargetKind::Org)
    );
}

#[test]
fn a_cursor_reads_back_from_its_token() {
    let cursor = Cursor {
        at_unix_nanos: 1_700_000_000_000_000_042,
        id: EntryId::new(Uuid::from_u128(77)),
    };

    assert_eq!(Cursor::parse(&cursor.token()), Some(cursor));
}

#[test]
fn a_mangled_cursor_is_refused() {
    let token = Cursor {
        at_unix_nanos: 9,
        id: EntryId::new(Uuid::from_u128(1)),
    }
    .token();

    assert_eq!(Cursor::parse(""), None);
    assert_eq!(Cursor::parse(&token[1..]), None);
    assert_eq!(Cursor::parse(&format!("+{}", &token[1..])), None);
    assert_eq!(Cursor::parse(&token.replace('0', "g")), None);
}

#[test]
fn a_page_defaults_to_fifty_and_never_exceeds_two_hundred() {
    assert_eq!(PageSize::new(None).get(), DEFAULT_PAGE);
    assert_eq!(DEFAULT_PAGE, 50);
    assert_eq!(PageSize::new(Some(10_000)).get(), MAX_PAGE);
    assert_eq!(MAX_PAGE, 200);
    assert_eq!(PageSize::new(Some(0)).get(), 1);
}

#[test]
fn a_detail_keeps_its_facts_in_order() {
    let detail = Detail::default()
        .with("from", "Acme")
        .with("to", "Acme Inc")
        .with_maybe("team", None::<&str>);

    assert_eq!(detail.get("from"), Some(Some("Acme")));
    assert_eq!(detail.get("team"), Some(None));
    assert_eq!(detail.get("absent"), None);
    assert_eq!(
        detail
            .pairs()
            .iter()
            .map(|(key, _)| &**key)
            .collect::<Vec<_>>(),
        vec!["from", "to", "team"]
    );
}
