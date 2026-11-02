use uuid::Uuid;

use super::desk::StatusDesk;
use super::entities::{ComponentDraft, ComponentPatch, Link, PageDraft, PagePatch, StatusPage};
use super::errors::StatusError;
use super::value_objects::{
    Accent, ComponentId, ComponentName, ComponentStatus, Mode, PageName, ServiceName, Slug,
    StatusPageId, Theme,
};
use crate::access::key_ring_tests::{
    MINE, MemberOfMine, OTHER, OURS, OneProject, OneSession, Ring, THEIRS, a_project, an_org,
    scopes, token,
};
use crate::access::{KeyScope, Reader, Readers};
use crate::alerts::{
    AlertRules, AlertSeverity, Comparison, Condition, Definition, DefinitionParts, NoDataPolicy,
    RuleDraft, RuleId, RuleName, Signal, TraceMeasure, TraceSignal,
};
use crate::audit::{Action, TargetKind};
use crate::clock::UnixSeconds;
use crate::identity::AccountId;
use crate::organisation::{OrgRole, Whoami};
use crate::telemetry::TenantId;
use crate::testing::{AlertLedger, Notebook, StatusLedger};

const NOW: u64 = 1_700_000_000;

type Desk<'a> = StatusDesk<
    OneSession,
    MemberOfMine,
    &'a Ring,
    OneProject,
    AlertLedger,
    StatusLedger,
    StatusLedger,
    Notebook,
>;

struct World {
    alerts: AlertLedger,
    status: StatusLedger,
    notebook: Notebook,
    keys: Ring,
}

impl World {
    fn new() -> Self {
        Self {
            alerts: AlertLedger::default(),
            status: StatusLedger::default(),
            notebook: Notebook::default(),
            keys: Ring::default(),
        }
    }

    fn desk(&self, role: OrgRole) -> Desk<'_> {
        StatusDesk::new(
            Readers::new(Whoami::new(OneSession, MemberOfMine(role)), &self.keys),
            OneProject,
            self.alerts.clone(),
            self.status.clone(),
            self.status.clone(),
            self.notebook.clone(),
        )
    }

    async fn a_rule(&self, org: TenantId) -> RuleId {
        AlertRules::create(
            &self.alerts,
            org,
            &RuleDraft {
                name: RuleName::try_from("Checkout errors").expect("a name"),
                project: None,
                enabled: true,
                definition: Definition::new(DefinitionParts {
                    signal: Signal::Traces(TraceSignal {
                        service: Some("checkout".into()),
                        operation: None,
                        environment: None,
                        measure: TraceMeasure::ErrorRate,
                    }),
                    condition: Condition::new(Comparison::Above, 5.0).expect("a threshold"),
                    window_secs: 300,
                    pending_secs: 0,
                    severity: AlertSeverity::Critical,
                    no_data: NoDataPolicy::Ok,
                    channels: Vec::new(),
                    page: None,
                })
                .expect("a definition"),
            },
            AccountId::new(Uuid::from_u128(9)),
        )
        .await
        .expect("kept")
        .expect("fresh")
        .id
    }

    async fn a_page(&self, slug: &str) -> StatusPageId {
        self.desk(OrgRole::Admin)
            .create(&session(), an_org(MINE), &draft(slug), at(0))
            .await
            .expect("created")
            .page
            .id
    }
}

fn session() -> Reader {
    Reader::Session(token())
}

fn at(seconds: u64) -> UnixSeconds {
    UnixSeconds::new(NOW + seconds)
}

fn draft(slug: &str) -> PageDraft {
    PageDraft {
        name: PageName::try_from("Acme").expect("a name"),
        slug: Slug::try_from(slug).expect("a slug"),
        public: false,
        hide_from_search: false,
        theme: Theme::default(),
        intro: None,
    }
}

fn manual(name: &str) -> ComponentDraft {
    ComponentDraft {
        name: ComponentName::try_from(name).expect("a name"),
        link: Link::Manual,
        manual_status: None,
        public: true,
    }
}

fn actions(world: &World) -> Vec<(Action, TargetKind)> {
    world
        .notebook
        .entries()
        .into_iter()
        .map(|entry| (entry.action, entry.target.kind))
        .collect()
}

#[tokio::test]
async fn an_administrator_creates_a_page_that_members_list_and_read() {
    let world = World::new();

    let created = world
        .desk(OrgRole::Admin)
        .create(&session(), an_org(MINE), &draft("acme"), at(5))
        .await
        .expect("created");

    assert_eq!(created.page.slug.as_str(), "acme");
    assert_eq!(created.page.created_ms, (NOW + 5) * 1_000);
    assert!(created.components.is_empty());
    let listed = world
        .desk(OrgRole::Member)
        .list(&session(), an_org(MINE))
        .await
        .expect("listed");
    assert_eq!(listed.len(), 1);
    let read = world
        .desk(OrgRole::Member)
        .get(&session(), an_org(MINE), created.page.id)
        .await
        .expect("read");
    assert_eq!(read.page, created.page);
    assert_eq!(
        actions(&world),
        vec![(Action::StatusPageCreated, TargetKind::StatusPage)]
    );
}

#[tokio::test]
async fn a_slug_is_unique_across_every_organisation() {
    let world = World::new();
    world.status.put_page(StatusPage {
        id: StatusPageId::fresh(),
        org: an_org(THEIRS),
        name: PageName::try_from("Theirs").expect("a name"),
        slug: Slug::try_from("taken").expect("a slug"),
        public: true,
        hide_from_search: false,
        theme: Theme::default(),
        intro: None,
        created_ms: 1,
        updated_ms: 1,
    });
    let mine = world.a_page("mine").await;
    let desk = world.desk(OrgRole::Admin);

    let created = desk
        .create(&session(), an_org(MINE), &draft("taken"), at(1))
        .await;
    let renamed = desk
        .update(
            &session(),
            an_org(MINE),
            mine,
            PagePatch {
                slug: Some(Slug::try_from("taken").expect("a slug")),
                ..PagePatch::default()
            },
            at(1),
        )
        .await;

    assert_eq!(created.map(|_| ()), Err(StatusError::SlugTaken));
    assert_eq!(renamed.map(|_| ()), Err(StatusError::SlugTaken));
    assert_eq!(world.status.pages().len(), 2);
}

#[tokio::test]
async fn an_update_changes_the_named_fields_once_and_audits_them() {
    let world = World::new();
    let id = world.a_page("acme").await;
    let desk = world.desk(OrgRole::Admin);
    let patch = PagePatch {
        name: Some(PageName::try_from("Acme Inc").expect("a name")),
        public: Some(true),
        hide_from_search: Some(true),
        theme: Some(Theme {
            accent: Accent::Teal,
            mode: Mode::Dark,
        }),
        intro: Some(Some(
            super::value_objects::Intro::try_from("Live status").expect("an intro"),
        )),
        ..PagePatch::default()
    };

    let changed = desk
        .update(&session(), an_org(MINE), id, patch.clone(), at(9))
        .await
        .expect("changed");
    desk.update(&session(), an_org(MINE), id, patch, at(10))
        .await
        .expect("unchanged");

    assert_eq!(changed.page.name.as_str(), "Acme Inc");
    assert!(changed.page.public && changed.page.hide_from_search);
    assert_eq!(changed.page.theme.accent, Accent::Teal);
    assert_eq!(changed.page.updated_ms, (NOW + 9) * 1_000);
    let updated: Vec<_> = world
        .notebook
        .entries()
        .into_iter()
        .filter(|entry| entry.action == Action::StatusPageUpdated)
        .collect();
    assert_eq!(
        updated.len(),
        1,
        "a patch that changes nothing is not audited"
    );
    assert_eq!(
        updated[0].detail.get("fields"),
        Some(Some("name,public,hide_from_search,theme,intro"))
    );
}

#[tokio::test]
async fn only_an_administrator_writes_a_read_key_only_reads_and_a_stranger_sees_nothing() {
    let world = World::new();
    let id = world.a_page("acme").await;
    let key = Reader::Key(
        world
            .keys
            .holding(an_org(MINE), None, scopes(&[KeyScope::Read])),
    );

    let member = world
        .desk(OrgRole::Member)
        .create(&session(), an_org(MINE), &draft("other"), at(1))
        .await;
    let keyed = world
        .desk(OrgRole::Admin)
        .delete(&key, an_org(MINE), id)
        .await;
    let read = world
        .desk(OrgRole::Member)
        .get(&key, an_org(MINE), id)
        .await;
    let stranger = world
        .desk(OrgRole::Admin)
        .get(&session(), an_org(OTHER), id)
        .await;
    let foreign = world
        .desk(OrgRole::Admin)
        .delete(&session(), an_org(OTHER), id)
        .await;

    assert_eq!(member.map(|_| ()), Err(StatusError::NotAdmin));
    assert_eq!(keyed, Err(StatusError::ReadOnly));
    assert!(read.is_ok());
    assert_eq!(stranger.map(|_| ()), Err(StatusError::NotYours));
    assert_eq!(foreign, Err(StatusError::NotYours));
    assert_eq!(world.status.pages().len(), 1);
}

#[tokio::test]
async fn components_are_added_in_order_reordered_changed_and_removed() {
    let world = World::new();
    let page = world.a_page("acme").await;
    let rule = world.a_rule(an_org(MINE)).await;
    let desk = world.desk(OrgRole::Admin);

    let website = desk
        .add_component(&session(), an_org(MINE), page, &manual("Website"))
        .await
        .expect("added");
    let checkout = desk
        .add_component(
            &session(),
            an_org(MINE),
            page,
            &ComponentDraft {
                link: Link::Service {
                    name: ServiceName::try_from("checkout-api").expect("a service"),
                    project: Some(a_project(OURS)),
                },
                ..manual("Checkout")
            },
        )
        .await
        .expect("added");
    let payments = desk
        .add_component(
            &session(),
            an_org(MINE),
            page,
            &ComponentDraft {
                link: Link::Alert(rule),
                ..manual("Payments")
            },
        )
        .await
        .expect("added");
    assert_eq!(
        [website.position, checkout.position, payments.position],
        [0, 1, 2]
    );

    let ordered = desk
        .order(
            &session(),
            an_org(MINE),
            page,
            &[payments.id, website.id, checkout.id],
        )
        .await
        .expect("ordered");
    assert_eq!(
        ordered
            .iter()
            .map(|component| component.name.as_str())
            .collect::<Vec<_>>(),
        ["Payments", "Website", "Checkout"]
    );
    let changed = desk
        .change_component(
            &session(),
            an_org(MINE),
            page,
            website.id,
            ComponentPatch {
                manual_status: Some(Some(ComponentStatus::Degraded)),
                public: Some(false),
                ..ComponentPatch::default()
            },
        )
        .await
        .expect("changed");
    assert_eq!(changed.manual_status, Some(ComponentStatus::Degraded));
    assert!(!changed.public);
    assert_eq!(changed.position, 1);
    desk.remove_component(&session(), an_org(MINE), page, checkout.id)
        .await
        .expect("removed");

    let sheet = desk
        .get(&session(), an_org(MINE), page)
        .await
        .expect("read");
    assert_eq!(
        sheet
            .components
            .iter()
            .map(|component| component.name.as_str())
            .collect::<Vec<_>>(),
        ["Payments", "Website"]
    );
    assert_eq!(
        actions(&world),
        vec![
            (Action::StatusPageCreated, TargetKind::StatusPage),
            (Action::StatusComponentCreated, TargetKind::StatusComponent),
            (Action::StatusComponentCreated, TargetKind::StatusComponent),
            (Action::StatusComponentCreated, TargetKind::StatusComponent),
            (Action::StatusPageUpdated, TargetKind::StatusPage),
            (Action::StatusComponentUpdated, TargetKind::StatusComponent),
            (Action::StatusComponentDeleted, TargetKind::StatusComponent),
        ]
    );
}

#[tokio::test]
async fn a_link_names_an_alert_and_a_project_of_the_organisation() {
    let world = World::new();
    let page = world.a_page("acme").await;
    let theirs = world.a_rule(an_org(THEIRS)).await;
    let desk = world.desk(OrgRole::Admin);

    let foreign_alert = desk
        .add_component(
            &session(),
            an_org(MINE),
            page,
            &ComponentDraft {
                link: Link::Alert(theirs),
                ..manual("Payments")
            },
        )
        .await;
    let foreign_project = desk
        .add_component(
            &session(),
            an_org(MINE),
            page,
            &ComponentDraft {
                link: Link::Service {
                    name: ServiceName::try_from("checkout").expect("a service"),
                    project: Some(a_project(OTHER)),
                },
                ..manual("Checkout")
            },
        )
        .await;

    assert_eq!(foreign_alert.map(|_| ()), Err(StatusError::NoSuchAlert));
    assert_eq!(foreign_project.map(|_| ()), Err(StatusError::NoSuchProject));
}

#[tokio::test]
async fn an_order_names_every_component_exactly_once() {
    let world = World::new();
    let page = world.a_page("acme").await;
    let desk = world.desk(OrgRole::Admin);
    let one = desk
        .add_component(&session(), an_org(MINE), page, &manual("One"))
        .await
        .expect("added");
    let two = desk
        .add_component(&session(), an_org(MINE), page, &manual("Two"))
        .await
        .expect("added");

    for asked in [
        vec![one.id],
        vec![one.id, one.id],
        vec![one.id, two.id, ComponentId::fresh()],
        vec![one.id, ComponentId::fresh()],
    ] {
        assert_eq!(
            desk.order(&session(), an_org(MINE), page, &asked)
                .await
                .map(|_| ()),
            Err(StatusError::Order)
        );
    }
}

#[tokio::test]
async fn deleting_a_page_is_audited_and_takes_its_components() {
    let world = World::new();
    let page = world.a_page("acme").await;
    let desk = world.desk(OrgRole::Admin);
    desk.add_component(&session(), an_org(MINE), page, &manual("One"))
        .await
        .expect("added");

    desk.delete(&session(), an_org(MINE), page)
        .await
        .expect("deleted");

    assert!(world.status.pages().is_empty());
    assert_eq!(
        desk.get(&session(), an_org(MINE), page).await.map(|_| ()),
        Err(StatusError::NoSuchPage)
    );
    assert_eq!(
        actions(&world).last(),
        Some(&(Action::StatusPageDeleted, TargetKind::StatusPage))
    );
}
