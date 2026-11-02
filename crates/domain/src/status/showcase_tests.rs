use std::cell::Cell;
use std::sync::Mutex;

use uuid::Uuid;

use super::entities::{Component, Link, StatusPage};
use super::errors::StatusError;
use super::showcase::Showcase;
use super::value_objects::{
    ComponentId, ComponentName, ComponentStatus, PageName, ServiceName, Slug, StatusPageId, Theme,
    UpdateKind,
};
use crate::alerts::{
    AlertRules, AlertSeverity, AlertStates, AlertStatus, Comparison, Condition, Definition,
    DefinitionParts, Labels, NoDataPolicy, RuleDraft, RuleId, RuleName, RuleState, SeriesState,
    Signal, TraceMeasure, TraceSignal,
};
use crate::catalogue::ProjectId;
use crate::clock::UnixSeconds;
use crate::explore::{
    ServiceFilter, ServiceSeries, ServiceSummary, SpanReader, TraceFilter, TraceSummary,
};
use crate::identity::AccountId;
use crate::incidents::{
    Attachment, AttachmentId, AttachmentKind, AttachmentLabel, Author, Change, CommentBody,
    Incident, IncidentId, IncidentSeverity, IncidentStatus, IncidentTimeline, IncidentTitle,
    Reference, TimelineEntry, TimelineId,
};
use crate::postmortems::{Postmortem, PostmortemBody, PostmortemId, PostmortemStatus, Postmortems};
use crate::store::StoreError;
use crate::telemetry::{Span, TenantId, TraceId};
use crate::testing::{AlertLedger, IncidentLedger, PostmortemLedger, StatusLedger};

const NOW: u64 = 1_700_000_000;
const NOW_MS: u64 = NOW * 1_000;
const DAY_MS: u64 = 86_400_000;
const PROJECT: u128 = 30;

fn org() -> TenantId {
    TenantId::new(Uuid::from_u128(1))
}

fn project() -> ProjectId {
    ProjectId::new(Uuid::from_u128(PROJECT))
}

#[derive(Default)]
struct Loads {
    held: Vec<(Option<ProjectId>, &'static str, u64, u64)>,
    asked: Mutex<Vec<ServiceFilter>>,
}

impl SpanReader for Loads {
    async fn services(
        &self,
        _tenant: TenantId,
        filter: &ServiceFilter,
    ) -> Result<Vec<ServiceSummary>, StoreError> {
        self.asked.lock().expect("the lock").push(filter.clone());
        Ok(self
            .held
            .iter()
            .filter(|(project, ..)| *project == filter.project)
            .map(|(_, name, requests, errors)| ServiceSummary {
                name: (*name).into(),
                requests: *requests,
                errors: *errors,
                p50_nanos: 0,
                p95_nanos: 0,
                p99_nanos: 0,
                last_seen_unix_nanos: 0,
            })
            .collect())
    }

    async fn operations(
        &self,
        _tenant: TenantId,
        _service: &str,
        _filter: &ServiceFilter,
    ) -> Result<Vec<ServiceSummary>, StoreError> {
        Ok(Vec::new())
    }

    async fn series(
        &self,
        _tenant: TenantId,
        _service: Option<&str>,
        _filter: &ServiceFilter,
    ) -> Result<ServiceSeries, StoreError> {
        Ok(ServiceSeries::default())
    }

    async fn traces(
        &self,
        _tenant: TenantId,
        _filter: &TraceFilter,
    ) -> Result<Vec<TraceSummary>, StoreError> {
        Ok(Vec::new())
    }

    async fn trace(
        &self,
        _tenant: TenantId,
        _id: TraceId,
        _project: Option<ProjectId>,
    ) -> Result<Vec<Span>, StoreError> {
        Ok(Vec::new())
    }
}

struct World {
    status: StatusLedger,
    incidents: IncidentLedger,
    postmortems: PostmortemLedger,
    alerts: AlertLedger,
    loads: Loads,
    placed: Cell<u32>,
}

type Shown<'a> = Showcase<
    'a,
    StatusLedger,
    IncidentLedger,
    IncidentLedger,
    PostmortemLedger,
    AlertLedger,
    AlertLedger,
    Loads,
>;

impl World {
    fn new() -> Self {
        Self {
            status: StatusLedger::default(),
            incidents: IncidentLedger::default(),
            postmortems: PostmortemLedger::default(),
            alerts: AlertLedger::default(),
            loads: Loads::default(),
            placed: Cell::new(0),
        }
    }

    fn showcase(&self) -> Shown<'_> {
        Showcase::new(
            self.status.clone(),
            self.incidents.clone(),
            self.incidents.clone(),
            self.postmortems.clone(),
            self.alerts.clone(),
            self.alerts.clone(),
            Some(&self.loads),
        )
    }

    fn page(&self, slug: &str, public: bool) -> StatusPageId {
        let id = StatusPageId::fresh();
        self.status.put_page(StatusPage {
            id,
            org: org(),
            name: PageName::try_from("Acme").expect("a name"),
            slug: Slug::try_from(slug).expect("a slug"),
            public,
            hide_from_search: true,
            theme: Theme::default(),
            intro: None,
            created_ms: 1,
            updated_ms: 1,
        });
        id
    }

    fn component(&self, page: StatusPageId, name: &str, link: Link, public: bool) {
        self.component_with(page, name, link, None, public);
    }

    fn component_with(
        &self,
        page: StatusPageId,
        name: &str,
        link: Link,
        manual_status: Option<ComponentStatus>,
        public: bool,
    ) {
        let position = self.placed.get();
        self.placed.set(position + 1);
        self.status.put_component(
            org(),
            Component {
                id: ComponentId::fresh(),
                page,
                name: ComponentName::try_from(name).expect("a name"),
                link,
                manual_status,
                position,
                public,
            },
        );
    }

    async fn firing(&self, severity: AlertSeverity) -> RuleId {
        let rule = AlertRules::create(
            &self.alerts,
            org(),
            &RuleDraft {
                name: RuleName::try_from("Payments errors").expect("a name"),
                project: None,
                enabled: true,
                definition: Definition::new(DefinitionParts {
                    signal: Signal::Traces(TraceSignal {
                        service: Some("payments".into()),
                        operation: None,
                        environment: None,
                        measure: TraceMeasure::ErrorRate,
                        placement: crate::placement::Placement::default(),
                    }),
                    condition: Condition::new(Comparison::Above, 5.0).expect("a threshold"),
                    window_secs: 300,
                    pending_secs: 0,
                    severity,
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
        .expect("fresh");
        AlertStates::keep(
            &self.alerts,
            rule.id,
            &SeriesState {
                labels: Labels::none(),
                state: RuleState {
                    status: AlertStatus::Firing,
                    value: Some(30.0),
                    since_ms: 1,
                    evaluated_ms: 1,
                },
            },
        )
        .await
        .expect("kept");
        rule.id
    }

    fn incident(
        &self,
        title: &str,
        public: bool,
        status: IncidentStatus,
        resolved_ms: Option<u64>,
    ) -> IncidentId {
        let id = IncidentId::fresh();
        self.incidents.put(Incident {
            id,
            org: org(),
            project: None,
            title: IncidentTitle::try_from(title).expect("a title"),
            severity: IncidentSeverity::Sev2,
            status,
            owner: Some(AccountId::new(Uuid::from_u128(9))),
            public,
            opened_ms: NOW_MS - DAY_MS,
            resolved_ms,
            opened_from_alert: None,
            paged_schedule: None,
        });
        id
    }

    async fn entry(&self, incident: IncidentId, change: Change, at_ms: u64) {
        IncidentTimeline::append(
            &self.incidents,
            &TimelineEntry {
                id: TimelineId::fresh(),
                incident,
                org: org(),
                author: Author::Account(AccountId::new(Uuid::from_u128(9))),
                change,
                at_ms,
            },
        )
        .await
        .expect("appended");
    }

    async fn postmortem(&self, incident: IncidentId, status: PostmortemStatus, body: &str) {
        Postmortems::create(
            &self.postmortems,
            &Postmortem {
                id: PostmortemId::fresh(),
                org: org(),
                incident,
                status,
                body: PostmortemBody::try_from(body).expect("a body"),
                author: None,
                edited: true,
                created_ms: 1,
                updated_ms: 2,
                published_ms: (status == PostmortemStatus::Published).then_some(3),
            },
        )
        .await
        .expect("kept");
    }
}

fn slug(text: &str) -> Slug {
    Slug::try_from(text).expect("a slug")
}

fn now() -> UnixSeconds {
    UnixSeconds::new(NOW)
}

fn comment(text: &str, public: bool) -> Change {
    Change::Comment {
        body: CommentBody::try_from(text).expect("a body"),
        public,
    }
}

async fn a_busy_page(world: &World) {
    let page = world.page("acme", true);
    let critical = world.firing(AlertSeverity::Critical).await;
    world.component(page, "Payments", Link::Alert(critical), true);
    world.component_with(
        page,
        "Payouts",
        Link::Manual,
        Some(ComponentStatus::Degraded),
        true,
    );
    world.component(page, "Internal admin", Link::Manual, false);
    let public = world.incident("Payments fail", true, IncidentStatus::Identified, None);
    let private = world.incident(
        "Secret db outage",
        false,
        IncidentStatus::Investigating,
        None,
    );
    world
        .entry(
            public,
            Change::Opened {
                title: IncidentTitle::try_from("Payments fail").expect("a title"),
                severity: IncidentSeverity::Sev2,
                alert: Some(critical),
            },
            NOW_MS - DAY_MS,
        )
        .await;
    world
        .entry(
            public,
            comment("db 10.0.0.4 full, ask ada@acme.dev", false),
            NOW_MS - 3,
        )
        .await;
    world
        .entry(public, comment("Some payments fail.", true), NOW_MS - 2)
        .await;
    world
        .entry(
            public,
            Change::Attachment(Attachment {
                id: AttachmentId::fresh(),
                incident: public,
                reference: Reference::parse(
                    AttachmentKind::Trace,
                    "0af7651916cd43dd8448eb211c80319c",
                )
                .expect("a trace"),
                label: AttachmentLabel::try_from("slow trace").expect("a label"),
                at_ms: NOW_MS - 1,
            }),
            NOW_MS - 1,
        )
        .await;
    world
        .entry(
            public,
            Change::Status {
                from: IncidentStatus::Investigating,
                to: IncidentStatus::Identified,
            },
            NOW_MS,
        )
        .await;
    world
        .entry(private, comment("Visible?", true), NOW_MS)
        .await;
}

#[tokio::test]
async fn spec_l2_7_a_public_page_shows_its_public_components_and_only_public_incidents() {
    let world = World::new();
    a_busy_page(&world).await;

    let view = world
        .showcase()
        .view(&slug("acme"), now())
        .await
        .expect("shown");

    assert_eq!(
        view.components
            .iter()
            .map(|component| (component.name.as_str(), component.status))
            .collect::<Vec<_>>(),
        vec![
            ("Payments", ComponentStatus::Down),
            ("Payouts", ComponentStatus::Degraded),
        ]
    );
    assert_eq!(view.overall, ComponentStatus::Down);
    assert!(view.hide_from_search);
    assert_eq!(view.incidents.len(), 1);
    let shown = &view.incidents[0];
    assert_eq!(shown.title.as_str(), "Payments fail");
    assert_eq!(
        shown
            .updates
            .iter()
            .map(|update| (update.kind, &*update.text))
            .collect::<Vec<_>>(),
        vec![
            (UpdateKind::Status, "identified"),
            (UpdateKind::Comment, "Some payments fail."),
            (UpdateKind::Opened, "investigating"),
        ]
    );
    let rendered = format!("{view:?}");
    for secret in [
        "10.0.0.4",
        "ada@acme.dev",
        "Secret db",
        "slow trace",
        "0af76519",
    ] {
        assert!(!rendered.contains(secret), "{secret} leaked: {rendered}");
    }
}

#[tokio::test]
async fn a_private_or_unknown_page_is_not_shown() {
    let world = World::new();
    world.page("hidden", false);

    for asked in ["hidden", "nobody"] {
        assert_eq!(
            world.showcase().view(&slug(asked), now()).await.map(|_| ()),
            Err(StatusError::NoSuchPage)
        );
    }
}

#[tokio::test]
async fn an_incident_resolved_more_than_seven_days_ago_leaves_the_page() {
    let world = World::new();
    world.page("acme", true);
    world.incident(
        "Recent",
        true,
        IncidentStatus::Resolved,
        Some(NOW_MS - 6 * DAY_MS),
    );
    world.incident(
        "Old",
        true,
        IncidentStatus::Resolved,
        Some(NOW_MS - 8 * DAY_MS),
    );

    let view = world
        .showcase()
        .view(&slug("acme"), now())
        .await
        .expect("shown");

    assert_eq!(
        view.incidents
            .iter()
            .map(|incident| incident.title.as_str())
            .collect::<Vec<_>>(),
        ["Recent"]
    );
    assert_eq!(
        view.overall,
        ComponentStatus::Operational,
        "a resolved incident leaves the page operational"
    );
}

#[tokio::test]
async fn a_service_link_reads_the_last_five_minutes_of_its_project() {
    let mut world = World::new();
    world.loads.held = vec![
        (Some(project()), "checkout", 100, 30),
        (None, "search", 100, 6),
        (None, "checkout", 100, 0),
    ];
    let page = world.page("acme", true);
    world.component(
        page,
        "Checkout",
        Link::Service {
            name: ServiceName::try_from("checkout").expect("a service"),
            project: Some(project()),
        },
        true,
    );
    world.component(
        page,
        "Search",
        Link::Service {
            name: ServiceName::try_from("search").expect("a service"),
            project: None,
        },
        true,
    );

    let view = world
        .showcase()
        .view(&slug("acme"), now())
        .await
        .expect("shown");

    assert_eq!(
        view.components
            .iter()
            .map(|component| component.status)
            .collect::<Vec<_>>(),
        [ComponentStatus::Down, ComponentStatus::Degraded]
    );
    let asked = world.loads.asked.lock().expect("the lock").clone();
    assert_eq!(asked.len(), 2, "one read per project");
    for filter in asked {
        assert_eq!(filter.window.to_unix_nanos(), NOW * 1_000_000_000);
        assert_eq!(filter.window.from_unix_nanos(), (NOW - 300) * 1_000_000_000);
    }
}

#[tokio::test]
async fn a_published_postmortem_of_a_public_incident_is_linked_and_served_redacted() {
    let world = World::new();
    world.page("acme", true);
    let published = world.incident(
        "Published",
        true,
        IncidentStatus::Resolved,
        Some(NOW_MS - DAY_MS),
    );
    let drafted = world.incident("Drafted", true, IncidentStatus::Investigating, None);
    let private = world.incident("Private", false, IncidentStatus::Resolved, Some(NOW_MS));
    world
        .postmortem(
            published,
            PostmortemStatus::Published,
            "# Published\nOwner: ada@acme.dev",
        )
        .await;
    world
        .postmortem(drafted, PostmortemStatus::Draft, "# Drafted")
        .await;
    world
        .postmortem(private, PostmortemStatus::Published, "# Private")
        .await;

    let view = world
        .showcase()
        .view(&slug("acme"), now())
        .await
        .expect("shown");
    let served = world
        .showcase()
        .postmortem(&slug("acme"), published)
        .await
        .expect("served");

    let linked: Vec<_> = view
        .incidents
        .iter()
        .map(|incident| (incident.title.as_str(), incident.postmortem))
        .collect();
    assert!(linked.contains(&("Published", true)));
    assert!(linked.contains(&("Drafted", false)));
    assert_eq!(&*served.body, "# Published\nOwner: [redacted]");
    assert_eq!(served.published_ms, 3);
    for refused in [drafted, private, IncidentId::fresh()] {
        assert_eq!(
            world
                .showcase()
                .postmortem(&slug("acme"), refused)
                .await
                .map(|_| ()),
            Err(StatusError::NoSuchPostmortem)
        );
    }
}
