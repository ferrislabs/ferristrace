use super::channel_book::ChannelBook;
use super::entities::ChannelChange;
use super::errors::{AlertError, BadChannel};
use super::value_objects::{ChannelKind, ChannelName, Destination};
use crate::access::key_ring_tests::{
    MINE, MemberOfMine, OneSession, Ring, THEIRS, an_org, scopes, token,
};
use crate::access::{KeyScope, Reader, Readers};
use crate::audit::{Action, TargetKind};
use crate::organisation::{OrgRole, Whoami};
use crate::testing::{AlertLedger, Couriered, Notebook};

type Book<'a> = ChannelBook<OneSession, MemberOfMine, &'a Ring, AlertLedger, Notebook>;

fn book<'a>(role: OrgRole, ledger: &AlertLedger, notebook: &Notebook, keys: &'a Ring) -> Book<'a> {
    ChannelBook::new(
        Readers::new(Whoami::new(OneSession, MemberOfMine(role)), keys),
        ledger.clone(),
        notebook.clone(),
    )
}

fn named(name: &str) -> ChannelName {
    ChannelName::try_from(name).expect("a name")
}

fn hook(url: &str) -> Destination {
    Destination::parse(ChannelKind::Webhook, url).expect("a url")
}

#[tokio::test]
async fn a_webhook_channel_hands_its_secret_back_once_and_keeps_it() {
    let (ledger, notebook, keys) = (AlertLedger::sealed(), Notebook::default(), Ring::default());
    let admin = book(OrgRole::Admin, &ledger, &notebook, &keys);

    let issued = admin
        .create(
            &token(),
            an_org(MINE),
            &named("pager"),
            &hook("https://hooks.example.com/in"),
            &Couriered::default(),
        )
        .await
        .expect("a channel");
    let listed = admin
        .list(&Reader::Session(token()), an_org(MINE))
        .await
        .expect("a list");

    let secret = issued.secret.expect("a secret");
    assert_eq!(secret.exposed().len(), 64);
    assert_eq!(ledger.secret_of(issued.channel.id), Some(secret));
    assert!(listed[0].has_secret);
    assert_eq!(notebook.entries()[0].action, Action::ChannelCreated);
    assert_eq!(notebook.entries()[0].target.kind, TargetKind::Channel);
    assert_eq!(
        notebook.entries()[0].detail.get("kind"),
        Some(Some("webhook"))
    );
}

#[tokio::test]
async fn an_email_channel_carries_no_secret() {
    let (ledger, notebook, keys) = (AlertLedger::default(), Notebook::default(), Ring::default());

    let issued = book(OrgRole::Admin, &ledger, &notebook, &keys)
        .create(
            &token(),
            an_org(MINE),
            &named("ops"),
            &Destination::parse(ChannelKind::Email, "ops@example.com").expect("an address"),
            &Couriered::default(),
        )
        .await
        .expect("a channel");

    assert!(issued.secret.is_none());
    assert!(!issued.channel.has_secret);
}

#[tokio::test]
async fn a_webhook_is_refused_without_a_seal_or_towards_a_private_address() {
    let (open, notebook, keys) = (AlertLedger::default(), Notebook::default(), Ring::default());
    let sealed = AlertLedger::sealed();

    let unsealed = book(OrgRole::Admin, &open, &notebook, &keys)
        .create(
            &token(),
            an_org(MINE),
            &named("pager"),
            &hook("https://hooks.example.com/in"),
            &Couriered::default(),
        )
        .await;
    let private = book(OrgRole::Admin, &sealed, &notebook, &keys)
        .create(
            &token(),
            an_org(MINE),
            &named("pager"),
            &hook("http://127.0.0.1:9000/in"),
            &Couriered::default(),
        )
        .await;
    let allowed = book(OrgRole::Admin, &sealed, &notebook, &keys)
        .create(
            &token(),
            an_org(MINE),
            &named("pager"),
            &hook("http://127.0.0.1:9000/in"),
            &Couriered::admitting_private(),
        )
        .await;

    assert_eq!(unsealed.err(), Some(AlertError::NoSecretKeeper));
    assert_eq!(private.err(), Some(AlertError::PrivateTarget));
    assert!(allowed.is_ok());
}

#[tokio::test]
async fn a_channel_is_renamed_retargeted_within_its_kind_and_deleted_by_an_admin_only() {
    let (ledger, notebook, keys) = (AlertLedger::sealed(), Notebook::default(), Ring::default());
    let admin = book(OrgRole::Admin, &ledger, &notebook, &keys);
    let hooks = Couriered::default();
    let id = admin
        .create(
            &token(),
            an_org(MINE),
            &named("pager"),
            &hook("https://a.example.com/"),
            &hooks,
        )
        .await
        .expect("a channel")
        .channel
        .id;

    let moved = admin
        .update(
            &token(),
            an_org(MINE),
            id,
            ChannelChange {
                name: Some(&named("paging")),
                target: Some("https://b.example.com/"),
                design: None,
            },
            &hooks,
        )
        .await
        .expect("an update");
    let wrong = admin
        .update(
            &token(),
            an_org(MINE),
            id,
            ChannelChange {
                name: None,
                target: Some("ops@example.com"),
                design: None,
            },
            &hooks,
        )
        .await;
    let member = book(OrgRole::Member, &ledger, &notebook, &keys)
        .delete(&token(), an_org(MINE), id)
        .await;
    let stranger = admin.delete(&token(), an_org(THEIRS), id).await;
    admin
        .delete(&token(), an_org(MINE), id)
        .await
        .expect("deleted");

    assert_eq!(moved.name.as_str(), "paging");
    assert_eq!(moved.destination.target(), "https://b.example.com/");
    assert!(moved.has_secret);
    assert_eq!(wrong.err(), Some(AlertError::Channel(BadChannel::Url)));
    assert_eq!(member.err(), Some(AlertError::NotYoursToChange));
    assert_eq!(stranger.err(), Some(AlertError::NotYours));
    let actions: Vec<Action> = notebook
        .entries()
        .iter()
        .map(|entry| entry.action)
        .collect();
    assert_eq!(
        actions,
        vec![
            Action::ChannelCreated,
            Action::ChannelUpdated,
            Action::ChannelDeleted
        ]
    );
}

#[tokio::test]
async fn a_read_key_lists_channels() {
    let (ledger, notebook, keys) = (AlertLedger::default(), Notebook::default(), Ring::default());
    let key = Reader::Key(keys.holding(an_org(MINE), None, scopes(&[KeyScope::Read])));

    let listed = book(OrgRole::Member, &ledger, &notebook, &keys)
        .list(&key, an_org(MINE))
        .await;

    assert_eq!(listed, Ok(Vec::new()));
}

#[tokio::test]
async fn a_chat_channel_needs_no_seal_and_carries_no_secret() {
    let (ledger, notebook, keys) = (AlertLedger::default(), Notebook::default(), Ring::default());

    let issued = book(OrgRole::Admin, &ledger, &notebook, &keys)
        .create(
            &token(),
            an_org(MINE),
            &named("team-chat"),
            &Destination::parse(
                ChannelKind::Slack,
                "https://hooks.slack.com/services/T000/B000/XXXX",
            )
            .expect("a slack hook"),
            &Couriered::default(),
        )
        .await
        .expect("a channel");

    assert!(issued.secret.is_none());
    assert!(!issued.channel.has_secret);
    assert_eq!(
        notebook.entries()[0].detail.get("kind"),
        Some(Some("slack"))
    );
}
