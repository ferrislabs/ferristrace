use super::ack::{AckClaim, AckKey};
use super::desk::PageDesk;
use super::entities::{Page, PageSource, Paged};
use super::errors::{AckRefused, OncallError};
use super::fixtures::{ADA, ME, at, crew, oncall, person, weekly};
use super::paging::{Pager, Paging};
use super::value_objects::{ESCALATE_AFTER_SECS, PageScope, PageState, ScheduleName};
use crate::access::key_ring_tests::{
    MINE, MemberOfMine, OneSession, Ring, THEIRS, an_org, scopes, token,
};
use crate::access::{KeyScope, Reader, Readers};
use crate::agents::{AgentCall, AgentScope, Delegation, VerifiedToken};
use crate::audit::{Action, Actor, TargetKind};
use crate::incidents::{Author, Change, IncidentId};
use crate::organisation::{OrgRole, Whoami};
use crate::testing::{AlertLedger, Couriered, Crew, IncidentLedger, Notebook, OncallLedger};

type Desk<'a> = PageDesk<
    OneSession,
    MemberOfMine,
    &'a Ring,
    Crew,
    OncallLedger,
    OncallLedger,
    IncidentLedger,
    Notebook,
>;

struct World {
    oncall: OncallLedger,
    incidents: IncidentLedger,
    notebook: Notebook,
    keys: Ring,
    key: AckKey,
}

impl World {
    fn new() -> Self {
        Self {
            oncall: oncall(),
            incidents: IncidentLedger::default(),
            notebook: Notebook::default(),
            keys: Ring::default(),
            key: AckKey::derive(&[5_u8; 32]),
        }
    }

    fn desk(&self, role: OrgRole) -> Desk<'_> {
        PageDesk::new(
            Readers::new(Whoami::new(OneSession, MemberOfMine(role)), &self.keys),
            crew(),
            self.oncall.clone(),
            self.oncall.clone(),
            self.incidents.clone(),
            self.notebook.clone(),
        )
    }

    async fn paged(&self, seconds: u64) -> Page {
        Paging::new(self.oncall.clone(), self.oncall.clone())
            .raise(
                an_org(MINE),
                weekly().id,
                PageSource::Incident(IncidentId::fresh()),
                "Checkout down",
                (super::fixtures::START + seconds) * 1_000,
            )
            .await
            .expect("asked")
            .expect("raised")
    }

    fn link(&self, page: &Page, account: u128, expires: u64) -> Box<str> {
        self.key.sign(
            &AckClaim {
                org: page.org,
                page: page.id,
                account: person(account),
            },
            at(expires),
        )
    }

    fn acknowledgements(&self) -> Vec<crate::audit::AuditEntry> {
        self.notebook
            .entries()
            .into_iter()
            .filter(|entry| entry.action == Action::PageAcknowledged)
            .collect()
    }
}

fn session() -> Reader {
    Reader::Session(token())
}

#[tokio::test]
async fn the_paged_person_acknowledges_in_the_app_and_it_lands_in_the_timeline_and_the_log() {
    let world = World::new();
    let page = world.paged(0).await;

    let acknowledged = world
        .desk(OrgRole::Member)
        .acknowledge(&session(), an_org(MINE), page.id, at(120))
        .await
        .expect("the target acknowledges");

    assert_eq!(acknowledged.page.state, PageState::Acknowledged);
    assert_eq!(acknowledged.page.acknowledged_by, Some(person(ME)));
    assert_eq!(acknowledged.page.escalate_at_ms, None);
    assert_eq!(acknowledged.target.account, person(ME));
    assert_eq!(
        acknowledged.schedule.as_ref().map(ScheduleName::as_str),
        Some("Primary")
    );
    let PageSource::Incident(incident) = page.source else {
        panic!("an incident page");
    };
    let told: Vec<_> = world
        .incidents
        .timeline()
        .into_iter()
        .map(|entry| (entry.incident, entry.author, entry.change))
        .collect();
    assert_eq!(
        told,
        vec![(
            incident,
            Author::Account(person(ME)),
            Change::Acknowledged { page: page.id }
        )]
    );
    let logged = world.acknowledgements();
    assert_eq!(logged.len(), 1);
    assert_eq!(logged[0].actor, Actor::Account(person(ME)));
    assert_eq!(logged[0].target.kind, TargetKind::Page);
    assert_eq!(logged[0].detail.get("via"), Some(Some("app")));
}

#[tokio::test]
async fn acknowledging_twice_is_fine_and_recorded_once() {
    let world = World::new();
    let page = world.paged(0).await;
    let desk = world.desk(OrgRole::Member);

    desk.acknowledge(&session(), an_org(MINE), page.id, at(60))
        .await
        .expect("once");
    let twice = desk
        .acknowledge(&session(), an_org(MINE), page.id, at(90))
        .await
        .expect("twice");

    assert_eq!(
        twice.page.acknowledged_ms,
        Some((super::fixtures::START + 60) * 1_000)
    );
    assert_eq!(world.acknowledgements().len(), 1);
    assert_eq!(world.incidents.timeline().len(), 1);
}

#[tokio::test]
async fn a_member_acknowledges_only_their_own_pages_and_an_administrator_any() {
    let world = World::new();
    let page = world.paged(7 * 86_400).await;
    assert_eq!(page.target, person(ADA));

    let member = world
        .desk(OrgRole::Member)
        .acknowledge(&session(), an_org(MINE), page.id, at(7 * 86_400 + 1))
        .await;
    let admin = world
        .desk(OrgRole::Admin)
        .acknowledge(&session(), an_org(MINE), page.id, at(7 * 86_400 + 2))
        .await
        .expect("an administrator acknowledges any page");

    assert_eq!(member.map(|_| ()), Err(OncallError::NotYourPage));
    assert_eq!(admin.page.acknowledged_by, Some(person(ME)));
}

#[tokio::test]
async fn a_read_key_lists_pages_but_never_acknowledges() {
    let world = World::new();
    let page = world.paged(0).await;
    let key = Reader::Key(
        world
            .keys
            .holding(an_org(MINE), None, scopes(&[KeyScope::Read])),
    );
    let desk = world.desk(OrgRole::Member);

    let listed = desk
        .list(&key, an_org(MINE), PageScope::Open)
        .await
        .expect("a read key reads");
    let refused = desk.acknowledge(&key, an_org(MINE), page.id, at(1)).await;

    assert_eq!(listed.len(), 1);
    assert_eq!(refused.map(|_| ()), Err(OncallError::ReadOnly));
}

#[tokio::test]
async fn another_organisation_sees_no_page() {
    let world = World::new();
    let page = world.paged(0).await;
    let desk = world.desk(OrgRole::Owner);

    let listed = desk.list(&session(), an_org(THEIRS), PageScope::All).await;
    let acknowledged = desk
        .acknowledge(&session(), an_org(THEIRS), page.id, at(1))
        .await;

    assert_eq!(listed.map(|_| ()), Err(OncallError::NotYours));
    assert_eq!(acknowledged.map(|_| ()), Err(OncallError::NotYours));
}

#[tokio::test]
async fn the_open_list_leaves_resolved_pages_out() {
    let world = World::new();
    let page = world.paged(0).await;
    Paging::new(world.oncall.clone(), world.oncall.clone())
        .resolve(an_org(MINE), page.source.origin(), 1)
        .await
        .expect("resolved");
    let desk = world.desk(OrgRole::Member);

    let open = desk
        .list(&session(), an_org(MINE), PageScope::Open)
        .await
        .expect("listed");
    let all = desk
        .list(&session(), an_org(MINE), PageScope::All)
        .await
        .expect("listed");

    assert!(open.is_empty());
    assert_eq!(all.len(), 1);
}

#[tokio::test]
async fn a_signed_link_acknowledges_as_its_recipient_without_a_session() {
    let world = World::new();
    let page = world.paged(0).await;
    let link = world.link(&page, ME, 3_600);

    let acknowledged = world
        .desk(OrgRole::Member)
        .acknowledge_link(&link, Some(&world.key), at(60))
        .await
        .expect("the link acknowledges");

    assert_eq!(acknowledged.state, PageState::Acknowledged);
    assert_eq!(acknowledged.acknowledged_by, Some(person(ME)));
    assert_eq!(
        world.acknowledgements()[0].detail.get("via"),
        Some(Some("link"))
    );
}

#[tokio::test]
async fn a_tampered_expired_or_unkeyed_link_acknowledges_nothing() {
    let world = World::new();
    let page = world.paged(0).await;
    let desk = world.desk(OrgRole::Member);
    let forged = AckKey::derive(&[6_u8; 32]).sign(
        &AckClaim {
            org: page.org,
            page: page.id,
            account: person(ME),
        },
        at(3_600),
    );
    let expired = world.link(&page, ME, 30);

    let tampered = desk
        .acknowledge_link(&forged, Some(&world.key), at(60))
        .await;
    let late = desk
        .acknowledge_link(&expired, Some(&world.key), at(60))
        .await;
    let unkeyed = desk
        .acknowledge_link(&world.link(&page, ME, 3_600), None, at(60))
        .await;

    assert_eq!(
        tampered.map(|_| ()),
        Err(OncallError::Ack(AckRefused::Tampered))
    );
    assert_eq!(late.map(|_| ()), Err(OncallError::Ack(AckRefused::Expired)));
    assert_eq!(unkeyed.map(|_| ()), Err(OncallError::NoAckKey));
    assert_eq!(world.oncall.pages()[0].state, PageState::Triggered);
    assert!(world.acknowledgements().is_empty());
}

#[tokio::test]
async fn spec_l2_5_an_acknowledged_page_stops_escalating() {
    let world = World::new();
    let page = world.paged(0).await;
    let alerts = AlertLedger::default();
    let mail = Couriered::default();
    let pager = || {
        Pager::new(
            Paging::new(world.oncall.clone(), world.oncall.clone()),
            alerts.clone(),
            crew(),
            world.incidents.clone(),
            world.notebook.clone(),
            &mail,
            None,
        )
    };
    pager().tick(at(0)).await.expect("a tick");
    world
        .desk(OrgRole::Member)
        .acknowledge(&session(), an_org(MINE), page.id, at(300))
        .await
        .expect("acknowledged");

    let mut ticks = Vec::new();
    for second in (330..=3 * ESCALATE_AFTER_SECS).step_by(30) {
        ticks.push(pager().tick(at(second)).await.expect("a tick"));
    }

    assert!(ticks.iter().all(|tick| matches!(
        tick,
        Paged::Worked {
            escalated: 0,
            sent: 0,
            ..
        }
    )));
    assert_eq!(world.oncall.pages()[0].target, person(ME));
    assert_eq!(mail.paged().len(), 1, "only the first email went out");
}

fn agent_calling(client: &str, claimed: &[&str], delegated: Option<Delegation>) -> Reader {
    Reader::Agent(AgentCall {
        token: VerifiedToken {
            client_id: client.into(),
            scopes: claimed.iter().map(|scope| (*scope).into()).collect(),
            jti: "jti-ack".into(),
            expires_at_unix_secs: 4_000_000_000,
            delegated,
        },
        route: "/api/orgs/{org}/pages/{id}/ack".into(),
    })
}

fn dana() -> Delegation {
    Delegation {
        subject: "sub-dana".into(),
        email: Some("dana@example.com".into()),
        act_sub: None,
        act_client_id: Some("mcp-gateway".into()),
    }
}

#[tokio::test]
async fn spec_l7_4_an_agent_with_the_scope_acknowledges_for_the_person_and_it_is_told_everywhere() {
    let world = World::new();
    let page = world.paged(0).await;
    let agent = world.keys.book.enrol(
        an_org(MINE),
        "gateway",
        &[AgentScope::TelemetryRead, AgentScope::OncallAcknowledge],
        None,
    );

    let acknowledged = world
        .desk(OrgRole::Member)
        .acknowledge(
            &agent_calling("gateway", &["oncall:acknowledge"], Some(dana())),
            an_org(MINE),
            page.id,
            at(60),
        )
        .await
        .expect("the agent acknowledges");

    assert_eq!(acknowledged.page.state, PageState::Acknowledged);
    assert_eq!(acknowledged.page.acknowledged_by, None);
    let uses = world.keys.book.uses();
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].1.actor, Actor::Agent(agent));
    assert_eq!(uses[0].1.action, Action::PageAcknowledged);
    assert_eq!(uses[0].1.detail.get("jti"), Some(Some("jti-ack")));
    assert_eq!(uses[0].1.detail.get("via"), Some(Some("agent")));
    assert_eq!(uses[0].1.detail.get("on_behalf_of"), Some(Some("sub-dana")));
    assert_eq!(
        uses[0].1.detail.get("on_behalf_of_email"),
        Some(Some("dana@example.com"))
    );
    let told = world.incidents.timeline();
    assert_eq!(told.len(), 1);
    assert_eq!(
        told[0].author,
        Author::Agent {
            id: Some(agent),
            name: "triage".into(),
            on_behalf_of: Some("dana@example.com".into()),
        }
    );
}

#[tokio::test]
async fn an_agent_without_the_acknowledge_scope_in_its_grant_or_its_token_is_refused() {
    let world = World::new();
    let page = world.paged(0).await;
    world
        .keys
        .book
        .enrol(an_org(MINE), "reader", &[AgentScope::TelemetryRead], None);
    world.keys.book.enrol(
        an_org(MINE),
        "gateway",
        &[AgentScope::OncallAcknowledge],
        None,
    );
    let desk = world.desk(OrgRole::Member);

    let not_granted = desk
        .acknowledge(
            &agent_calling("reader", &["oncall:acknowledge", "telemetry:read"], None),
            an_org(MINE),
            page.id,
            at(1),
        )
        .await;
    let not_claimed = desk
        .acknowledge(
            &agent_calling("gateway", &["telemetry:read"], None),
            an_org(MINE),
            page.id,
            at(1),
        )
        .await;

    assert_eq!(not_granted.map(|_| ()), Err(OncallError::OutOfScope));
    assert_eq!(not_claimed.map(|_| ()), Err(OncallError::OutOfScope));
    assert!(world.keys.book.uses().is_empty());
    assert!(world.incidents.timeline().is_empty());
}

#[tokio::test]
async fn an_agent_of_another_organisation_or_a_project_bound_one_acknowledges_nothing() {
    let world = World::new();
    let page = world.paged(0).await;
    world.keys.book.enrol(
        an_org(THEIRS),
        "foreign",
        &[AgentScope::OncallAcknowledge],
        None,
    );
    world.keys.book.enrol(
        an_org(MINE),
        "bound",
        &[AgentScope::OncallAcknowledge],
        Some(crate::access::key_ring_tests::a_project(1)),
    );
    let desk = world.desk(OrgRole::Member);

    let foreign = desk
        .acknowledge(
            &agent_calling("foreign", &["oncall:acknowledge"], None),
            an_org(MINE),
            page.id,
            at(1),
        )
        .await;
    let bound = desk
        .acknowledge(
            &agent_calling("bound", &["oncall:acknowledge"], None),
            an_org(MINE),
            page.id,
            at(1),
        )
        .await;

    assert_eq!(foreign.map(|_| ()), Err(OncallError::NoSession));
    assert_eq!(bound.map(|_| ()), Err(OncallError::OutOfScope));
    assert!(world.keys.book.uses().is_empty());
}

#[tokio::test]
async fn a_second_agent_acknowledgement_changes_nothing_and_is_logged_as_a_read() {
    let world = World::new();
    let page = world.paged(0).await;
    world.keys.book.enrol(
        an_org(MINE),
        "gateway",
        &[AgentScope::OncallAcknowledge],
        None,
    );
    let desk = world.desk(OrgRole::Member);
    let agent = agent_calling("gateway", &["oncall:acknowledge"], Some(dana()));

    desk.acknowledge(&agent, an_org(MINE), page.id, at(1))
        .await
        .expect("once");
    desk.acknowledge(&agent, an_org(MINE), page.id, at(2))
        .await
        .expect("twice");

    let uses = world.keys.book.uses();
    assert_eq!(uses.len(), 2);
    assert_eq!(uses[1].1.action, Action::AgentRead);
    assert_eq!(uses[1].1.detail.get("on_behalf_of"), Some(Some("sub-dana")));
    assert_eq!(world.incidents.timeline().len(), 1);
}

#[tokio::test]
async fn an_agent_never_does_another_oncall_write_through_the_desk() {
    let world = World::new();
    let page = world.paged(0).await;
    world
        .keys
        .book
        .enrol(an_org(MINE), "gateway", &[AgentScope::TelemetryRead], None);

    let listed = world
        .desk(OrgRole::Member)
        .list(
            &agent_calling("gateway", &["telemetry:read"], None),
            an_org(MINE),
            PageScope::Open,
        )
        .await
        .expect("an agent with telemetry:read lists pages");

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].page.id, page.id);
}
