use std::io::Read;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::header::{AUTHORIZATION, CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::{Method, Request, StatusCode};
use ferristrace_api::{AppState, export_router, key_router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{Bearers, Inert, Notebook, Untransacted};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    AccountId, Action, Actor, ApiKey, ApiKeys, ApiSecret, Assigned, Created, ExportSource,
    ExportedSpan, IssuedKey, KeyId, KeyName, KeyScopes, Membership, Memberships, NoPhotos, OrgRole,
    Organisation, Project, ProjectId, ProjectName, Projects, ReadGrant, Renamed, ServiceFilter,
    ServiceSeries, ServiceSummary, Session, SessionError, SessionStore, SessionToken, Snapshot,
    Span, SpanCursor, SpanEvent, SpanId, SpanKind, SpanPage, SpanPages, SpanReader, SpanStatus,
    StoreError, TeamId, TenantId, TraceFilter, TraceId, TraceSummary, UnixSeconds, WhoamiError,
};
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "ft.person.0123456789";
const MINE: u128 = 1;
const THEIRS: u128 = 2;
const ME: u128 = 9;
const EMAIL: &str = "person@example.com";

fn id(seed: u128) -> String {
    Uuid::from_u128(seed).to_string()
}

struct OneSession;

impl SessionStore for OneSession {
    async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
        if token.identity().subject.as_str() == TOKEN {
            Ok(Session {
                account: AccountId::new(Uuid::from_u128(ME)),
                email: EMAIL.try_into().expect("an address"),
                name: None,
                photo: None,
                org: None,
                expires_at: UnixSeconds::new(0),
            })
        } else {
            Err(SessionError::Unknown)
        }
    }
}

struct MemberOfMine(OrgRole);

impl Memberships for MemberOfMine {
    async fn of(&self, _account: AccountId) -> Result<Vec<Membership>, WhoamiError> {
        Ok(vec![Membership {
            org: TenantId::new(Uuid::from_u128(MINE)),
            name: "Acme".into(),
            role: self.0,
        }])
    }
}

#[derive(Default)]
struct Vault {
    held: Mutex<Vec<(TenantId, ApiKey, String, bool)>>,
}

impl ApiKeys for Vault {
    async fn list(&self, org: TenantId) -> Result<Vec<ApiKey>, StoreError> {
        Ok(self
            .held
            .lock()
            .expect("the lock")
            .iter()
            .filter(|(held, _, _, revoked)| *held == org && !revoked)
            .map(|(_, key, _, _)| key.clone())
            .collect())
    }

    async fn issue(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
        name: &KeyName,
        scopes: &KeyScopes,
        by: AccountId,
    ) -> Result<IssuedKey, StoreError> {
        let mut held = self.held.lock().expect("the lock");
        let seed = u128::try_from(held.len()).expect("a count") + 100;
        let prefix = format!("ft_live_{seed:012x}");
        let plaintext = format!("{prefix}.{seed:064x}");
        let key = ApiKey {
            id: KeyId::new(Uuid::from_u128(seed)),
            project,
            name: name.as_str().into(),
            prefix: prefix.into(),
            scopes: scopes.clone(),
            created_at_unix_nanos: 1,
            created_by: Some(by),
            last_used_at_unix_nanos: None,
        };
        held.push((org, key.clone(), plaintext.clone(), false));
        Ok(IssuedKey {
            key,
            secret: ApiSecret::from_plaintext(&plaintext),
        })
    }

    async fn revoke(&self, org: TenantId, id: KeyId) -> Result<bool, StoreError> {
        let mut held = self.held.lock().expect("the lock");
        Ok(held
            .iter_mut()
            .find(|(held, key, _, revoked)| *held == org && key.id == id && !revoked)
            .map(|(_, _, _, revoked)| *revoked = true)
            .is_some())
    }

    async fn grant_for(&self, secret: &ApiSecret) -> Result<Option<ReadGrant>, StoreError> {
        Ok(self
            .held
            .lock()
            .expect("the lock")
            .iter()
            .find(|(_, _, plaintext, revoked)| plaintext == secret.exposed() && !revoked)
            .map(|(org, key, _, _)| ReadGrant {
                org: *org,
                project: key.project,
            }))
    }
}

struct NoProjects;

impl Projects for NoProjects {
    async fn of(&self, _tenant: TenantId) -> Result<Vec<Project>, StoreError> {
        Ok(Vec::new())
    }

    async fn create(&self, _org: TenantId, _name: &ProjectName) -> Result<Created, StoreError> {
        Ok(Created::Done)
    }

    async fn rename(
        &self,
        _org: TenantId,
        _project: ProjectId,
        _name: &ProjectName,
    ) -> Result<Renamed, StoreError> {
        Ok(Renamed::Gone)
    }

    async fn forget(&self, _org: TenantId, _project: ProjectId) -> Result<bool, StoreError> {
        Ok(false)
    }

    async fn assign(
        &self,
        _org: TenantId,
        _project: ProjectId,
        _team: Option<TeamId>,
    ) -> Result<Assigned, StoreError> {
        Ok(Assigned::Gone)
    }
}

struct Snapshots {
    vault: Arc<Vault>,
}

impl ExportSource for Snapshots {
    async fn snapshot(&self, org: TenantId) -> Result<Snapshot, StoreError> {
        Ok(Snapshot {
            org: Organisation {
                id: org,
                name: "Acme".into(),
                plan: "free".into(),
                created_at_unix_nanos: 5_000_000,
                logo: None,
                retention: None,
                retention_cap: None,
                timezone: ferristrace_domain::OrgTimezone::utc(),
            },
            projects: Vec::new(),
            teams: Vec::new(),
            members: Vec::new(),
            keys: self.vault.list(org).await?,
            decisions: Vec::new(),
            audit: Vec::new(),
        })
    }
}

const PER_PAGE: usize = 2;

struct Shelf {
    held: Vec<(TenantId, ExportedSpan)>,
}

fn a_span(seed: u8, service: &str, events: Vec<SpanEvent>) -> ExportedSpan {
    ExportedSpan {
        project: Some(ProjectId::new(Uuid::from_u128(77))),
        span: Span {
            trace_id: TraceId::try_from([seed; 16].as_slice()).expect("sixteen bytes"),
            span_id: SpanId::try_from([seed; 8].as_slice()).expect("eight bytes"),
            parent: (seed > 1).then(|| SpanId::try_from([1_u8; 8].as_slice()).expect("eight")),
            name: format!("work-{seed}").into(),
            service_name: service.into(),
            kind: SpanKind::Server,
            status: SpanStatus::Error,
            start_unix_nanos: 1_700_000_000_000_000_000 + u64::from(seed),
            end_unix_nanos: 1_700_000_001_000_000_000 + u64::from(seed),
            attributes: std::collections::BTreeMap::from([("http.route".into(), "/pay".into())]),
            events,
        },
    }
}

impl SpanPages for Shelf {
    async fn page(
        &self,
        org: TenantId,
        after: Option<&SpanCursor>,
        limit: u32,
    ) -> Result<SpanPage, StoreError> {
        let size = PER_PAGE.min(usize::try_from(limit).unwrap_or(usize::MAX));
        let mine: Vec<_> = self
            .held
            .iter()
            .filter(|(owner, _)| *owner == org)
            .map(|(_, row)| row.clone())
            .collect();
        let start = after.map_or(0, |cursor| {
            mine.iter()
                .position(|row| row.span.span_id == cursor.span_id())
                .map_or(mine.len(), |at| at + 1)
        });
        let probed = mine.into_iter().skip(start).take(size + 1).collect();
        Ok(SpanPage::cut(
            probed,
            u32::try_from(size).unwrap_or(u32::MAX),
        ))
    }
}

impl ferristrace_domain::PlacementReader for Shelf {
    async fn placements(
        &self,
        _tenant: TenantId,
        _window: ferristrace_domain::Window,
        _project: Option<ProjectId>,
    ) -> Result<ferristrace_domain::Placements, StoreError> {
        Ok(ferristrace_domain::Placements::default())
    }
}

impl ferristrace_domain::CulpritReader for Shelf {
    async fn culprits(
        &self,
        _org: ferristrace_domain::TenantId,
        _project: Option<ferristrace_domain::ProjectId>,
        _signal: &ferristrace_domain::Signal,
        _window: ferristrace_domain::Window,
    ) -> Result<ferristrace_domain::Culprits, ferristrace_domain::StoreError> {
        Ok(ferristrace_domain::Culprits::default())
    }
}

impl SpanReader for Shelf {
    async fn services(
        &self,
        _tenant: TenantId,
        _filter: &ServiceFilter,
    ) -> Result<Vec<ServiceSummary>, StoreError> {
        Ok(Vec::new())
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

struct Fakes {
    role: OrgRole,
    vault: Arc<Vault>,
    notebook: Notebook,
}

impl Untransacted for Fakes {}

impl RepoFor<domain::Sessions> for Fakes {
    type Repo = OneSession;

    fn build(&self, (): &()) -> Self::Repo {
        OneSession
    }
}

impl RepoFor<domain::Memberships> for Fakes {
    type Repo = MemberOfMine;

    fn build(&self, (): &()) -> Self::Repo {
        MemberOfMine(self.role)
    }
}

impl RepoFor<domain::ApiKeys> for Fakes {
    type Repo = Arc<Vault>;

    fn build(&self, (): &()) -> Self::Repo {
        Arc::clone(&self.vault)
    }
}

impl RepoFor<domain::Projects> for Fakes {
    type Repo = NoProjects;

    fn build(&self, (): &()) -> Self::Repo {
        NoProjects
    }
}

impl RepoFor<domain::Audit> for Fakes {
    type Repo = Notebook;

    fn build(&self, (): &()) -> Self::Repo {
        self.notebook.clone()
    }
}

impl RepoFor<domain::Export> for Fakes {
    type Repo = Snapshots;

    fn build(&self, (): &()) -> Self::Repo {
        Snapshots {
            vault: Arc::clone(&self.vault),
        }
    }
}

struct TestStack;

impl Stack for TestStack {
    type Repos = Fakes;
    type Clock = Inert;
    type Mailer = Inert;
    type Photos = NoPhotos;
    type SpanStore = Shelf;
    type IssueStore = Inert;
    type LogStore = Inert;
    type MetricStore = Inert;
    type Meter = Inert;
    type Users = Bearers;
    type Agents = Inert;
    type Sink = Inert;
    type Quota = Inert;
    type Directory = Inert;
    type Webhooks = Inert;
}

struct Harness {
    vault: Arc<Vault>,
    notebook: Notebook,
}

struct Served {
    status: StatusCode,
    content_type: Option<String>,
    disposition: Option<String>,
    body: Vec<u8>,
}

impl Harness {
    fn new() -> Self {
        let notebook = Notebook::default();
        notebook.label(Actor::Account(AccountId::new(Uuid::from_u128(ME))), EMAIL);
        Self {
            vault: Arc::default(),
            notebook,
        }
    }

    fn shelf() -> Shelf {
        let ours = TenantId::new(Uuid::from_u128(MINE));
        let theirs = TenantId::new(Uuid::from_u128(THEIRS));
        Shelf {
            held: vec![
                (
                    ours,
                    a_span(
                        1,
                        "checkout",
                        vec![SpanEvent {
                            time_unix_nanos: 1_700_000_000_000_000_500,
                            name: "exception".into(),
                            attributes: std::collections::BTreeMap::from([(
                                "exception.type".into(),
                                "Timeout".into(),
                            )]),
                        }],
                    ),
                ),
                (theirs, a_span(9, "theirs-service", Vec::new())),
                (ours, a_span(2, "checkout", Vec::new())),
                (ours, a_span(3, "payments", Vec::new())),
                (ours, a_span(4, "payments", Vec::new())),
                (theirs, a_span(8, "theirs-service", Vec::new())),
                (ours, a_span(5, "payments", Vec::new())),
            ],
        }
    }

    fn app(&self, role: OrgRole, span_store: Option<Shelf>) -> Router {
        let adapters = Adapters {
            clock: Inert::default(),
            mailer: Inert::default(),
            webhooks: Inert::default(),
            photos: NoPhotos,
            meter: Inert::default(),
            span_store,
            issue_store: None,
            log_store: None,
            metric_store: None,
            users: Some(Bearers),
        };
        let state = AppState::new(Arc::new(Ferristrace::<TestStack>::new(
            Fakes {
                role,
                vault: Arc::clone(&self.vault),
                notebook: self.notebook.clone(),
            },
            adapters,
        )));
        key_router(state.clone()).merge(export_router(state))
    }

    async fn served(&self, role: OrgRole, request: Request<Body>) -> Served {
        let response = self
            .app(role, Some(Self::shelf()))
            .oneshot(request)
            .await
            .expect("a response");
        let status = response.status();
        let header = |name| {
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned)
        };
        let content_type = header(CONTENT_TYPE);
        let disposition = header(CONTENT_DISPOSITION);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        Served {
            status,
            content_type,
            disposition,
            body: body.to_vec(),
        }
    }

    async fn issued(&self, name: &str) -> serde_json::Value {
        let served = self
            .served(
                OrgRole::Admin,
                as_session(
                    Method::POST,
                    &format!("/api/orgs/{}/keys", id(MINE)),
                    Some(&format!(
                        r#"{{"name":"{name}","scopes":["ingest","read"]}}"#
                    )),
                ),
            )
            .await;
        assert_eq!(served.status, StatusCode::CREATED);
        serde_json::from_slice(&served.body).expect("json")
    }

    async fn exported(&self, role: OrgRole, org: u128) -> Served {
        self.served(role, downloading(org)).await
    }
}

fn as_session(method: Method, uri: &str, body: Option<&str>) -> Request<Body> {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {TOKEN}"));
    match body {
        Some(json) => request
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(json.to_owned())),
        None => request.body(Body::empty()),
    }
    .expect("a request")
}

fn downloading(org: u128) -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri(format!("/api/orgs/{}/export", id(org)))
        .header(AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .expect("a request")
}

fn unpacked(archive: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut entries = Vec::new();
    let decoder = flate2::read::GzDecoder::new(archive);
    for entry in tar::Archive::new(decoder).entries().expect("a tar") {
        let mut entry = entry.expect("an entry");
        let path = entry.path().expect("a path").to_string_lossy().into_owned();
        let mut content = Vec::new();
        entry.read_to_end(&mut content).expect("the content");
        entries.push((path, content));
    }
    entries
}

fn lines(content: &[u8]) -> Vec<serde_json::Value> {
    std::str::from_utf8(content)
        .expect("text")
        .lines()
        .map(|line| serde_json::from_str(line).expect("a json line"))
        .collect()
}

#[tokio::test]
async fn an_owner_downloads_a_gzip_tar_of_the_control_plane_then_numbered_span_pages() {
    let harness = Harness::new();

    let served = harness.exported(OrgRole::Owner, MINE).await;

    assert_eq!(served.status, StatusCode::OK);
    assert_eq!(served.content_type.as_deref(), Some("application/gzip"));
    assert_eq!(
        served.disposition,
        Some(format!(
            "attachment; filename=\"ferristrace-{}-19700101.tar.gz\"",
            id(MINE)
        ))
    );
    let entries = unpacked(&served.body);
    let names: Vec<_> = entries.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "control-plane.json",
            "spans/000001.ndjson",
            "spans/000002.ndjson",
            "spans/000003.ndjson",
        ]
    );
    let plane: serde_json::Value = serde_json::from_slice(&entries[0].1).expect("json");
    assert_eq!(plane["format"], "ferristrace-export");
    assert_eq!(plane["version"], 1);
    assert_eq!(plane["exported_at_ms"], 0);
    assert_eq!(plane["org"]["id"], id(MINE));
    assert_eq!(plane["org"]["name"], "Acme");
    assert_eq!(plane["org"]["plan"], "free");
    assert_eq!(plane["org"]["created_at_ms"], 5);
    assert_eq!(plane["org"]["retention_days"], serde_json::Value::Null);
    for field in [
        "projects",
        "teams",
        "members",
        "keys",
        "issue_decisions",
        "audit_entries",
    ] {
        assert!(plane[field].is_array(), "{field}");
    }
}

#[tokio::test]
async fn the_span_lines_carry_the_documented_shape_and_only_the_orgs_own_spans() {
    let harness = Harness::new();

    let served = harness.exported(OrgRole::Owner, MINE).await;

    let entries = unpacked(&served.body);
    let spans: Vec<serde_json::Value> = entries[1..]
        .iter()
        .flat_map(|(_, content)| lines(content))
        .collect();
    assert_eq!(spans.len(), 5);
    let services: Vec<_> = spans
        .iter()
        .map(|span| span["service_name"].as_str().expect("a service"))
        .collect();
    assert!(!services.contains(&"theirs-service"));
    let first = &spans[0];
    assert_eq!(first["trace_id"], "01".repeat(16));
    assert_eq!(first["span_id"], "01".repeat(8));
    assert_eq!(first["parent_span_id"], serde_json::Value::Null);
    assert_eq!(first["project_id"], id(77));
    assert_eq!(first["name"], "work-1");
    assert_eq!(first["kind"], "server");
    assert_eq!(first["status"], "error");
    assert_eq!(first["start_unix_nanos"], 1_700_000_000_000_000_001_u64);
    assert_eq!(first["end_unix_nanos"], 1_700_000_001_000_000_001_u64);
    assert_eq!(first["attributes"]["http.route"], "/pay");
    assert_eq!(first["events"][0]["name"], "exception");
    assert_eq!(
        first["events"][0]["time_unix_nanos"],
        1_700_000_000_000_000_500_u64
    );
    assert_eq!(
        first["events"][0]["attributes"]["exception.type"],
        "Timeout"
    );
    assert_eq!(spans[1]["parent_span_id"], "01".repeat(8));
    let ids: Vec<_> = spans
        .iter()
        .map(|span| span["span_id"].as_str().expect("an id").to_owned())
        .collect();
    assert_eq!(
        ids,
        (1..=5)
            .map(|seed| format!("{seed:02x}").repeat(8))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn an_export_audits_the_owner_and_nothing_else() {
    let harness = Harness::new();

    harness.exported(OrgRole::Owner, MINE).await;

    let entries = harness.notebook.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].action, Action::OrgExported);
    assert_eq!(
        entries[0].actor,
        Actor::Account(AccountId::new(Uuid::from_u128(ME)))
    );
}

#[tokio::test]
async fn the_control_plane_names_a_key_but_never_its_secret_or_digest() {
    let harness = Harness::new();
    let issued = harness.issued("ci").await;
    let secret = issued["secret"].as_str().expect("a secret").to_owned();
    let offered = secret
        .rsplit_once('.')
        .expect("a split secret")
        .1
        .to_owned();

    let served = harness.exported(OrgRole::Owner, MINE).await;

    assert_eq!(served.status, StatusCode::OK);
    let entries = unpacked(&served.body);
    let text = String::from_utf8(entries[0].1.clone()).expect("text");
    assert!(!text.contains(&secret));
    assert!(!text.contains(&offered));
    let plane: serde_json::Value = serde_json::from_str(&text).expect("json");
    let keys = plane["keys"].as_array().expect("keys");
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0]["name"], "ci");
    assert_eq!(keys[0]["scopes"], serde_json::json!(["ingest", "read"]));
    assert!(
        keys[0]["prefix"]
            .as_str()
            .expect("a prefix")
            .starts_with("ft_live_")
    );
    let mut fields: Vec<_> = keys[0]
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect();
    fields.sort();
    assert_eq!(
        fields,
        vec![
            "created_at_ms",
            "created_by",
            "id",
            "last_used_at_ms",
            "name",
            "prefix",
            "project",
            "scopes"
        ]
    );
}

#[tokio::test]
async fn an_admin_and_a_member_are_refused_and_nothing_is_audited() {
    for role in [OrgRole::Admin, OrgRole::Member] {
        let harness = Harness::new();

        let served = harness.exported(role, MINE).await;

        assert_eq!(served.status, StatusCode::FORBIDDEN);
        assert!(harness.notebook.entries().is_empty());
    }
}

#[tokio::test]
async fn someone_who_is_not_a_member_finds_no_such_organisation() {
    let harness = Harness::new();

    let served = harness.exported(OrgRole::Owner, THEIRS).await;

    assert_eq!(served.status, StatusCode::NOT_FOUND);
    assert!(harness.notebook.entries().is_empty());
}

#[tokio::test]
async fn only_a_persons_bearer_opens_an_export() {
    let harness = Harness::new();
    let key = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/orgs/{}/export", id(MINE)))
        .header(AUTHORIZATION, "Bearer ft_live_0123456789")
        .body(Body::empty())
        .expect("a request");
    let agent = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/orgs/{}/export", id(MINE)))
        .header(AUTHORIZATION, "Bearer ft.agent.0123456789")
        .body(Body::empty())
        .expect("a request");
    let bare = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/orgs/{}/export", id(MINE)))
        .body(Body::empty())
        .expect("a request");

    assert_eq!(
        harness.served(OrgRole::Owner, key).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        harness.served(OrgRole::Owner, agent).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        harness.served(OrgRole::Owner, bare).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert!(harness.notebook.entries().is_empty());
}

#[tokio::test]
async fn an_organisation_without_spans_is_exported_with_its_control_plane_alone() {
    let harness = Harness::new();

    let response = harness
        .app(OrgRole::Owner, Some(Shelf { held: Vec::new() }))
        .oneshot(downloading(MINE))
        .await
        .expect("a response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    let entries = unpacked(&body);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].0, "control-plane.json");
}

#[tokio::test]
async fn without_a_span_store_the_export_is_unavailable_and_unaudited() {
    let harness = Harness::new();

    let response = harness
        .app(OrgRole::Owner, None)
        .oneshot(downloading(MINE))
        .await
        .expect("a response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(harness.notebook.entries().is_empty());
}
