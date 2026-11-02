use super::derivation::{derive, overall, public_updates, redacted, shown};
use super::entities::{
    Link, Observed, PublicComponent, PublicIncident, PublicPostmortem, PublicView, ServiceLoad,
    StatusPage,
};
use super::errors::StatusError;
use super::ports::StatusPages;
use super::value_objects::{SERVICE_WINDOW_SECS, Slug, StatusPageId};
use crate::access::{ApiKeys, Reader, Readers};
use crate::alerts::{AlertRules, AlertStates, AlertStatus};
use crate::catalogue::ProjectId;
use crate::clock::UnixSeconds;
use crate::explore::{ServiceFilter, SpanReader, Window};
use crate::identity::SessionStore;
use crate::incidents::{IncidentFilter, IncidentId, IncidentScope, IncidentTimeline, Incidents};
use crate::organisation::Memberships;
use crate::postmortems::{PostmortemStatus, Postmortems};
use crate::telemetry::TenantId;

const MILLIS_PER_SECOND: u64 = 1_000;
const NANOS_PER_SECOND: u64 = 1_000_000_000;

#[derive(Debug)]
pub struct Showcase<'a, Pg, I, L, Pm, R, T, Sp> {
    pages: Pg,
    incidents: I,
    timeline: L,
    postmortems: Pm,
    rules: R,
    states: T,
    spans: Option<&'a Sp>,
}

impl<'a, Pg, I, L, Pm, R, T, Sp> Showcase<'a, Pg, I, L, Pm, R, T, Sp>
where
    Pg: StatusPages,
    I: Incidents,
    L: IncidentTimeline,
    Pm: Postmortems,
    R: AlertRules,
    T: AlertStates,
    Sp: SpanReader + Sync,
{
    pub fn new(
        pages: Pg,
        incidents: I,
        timeline: L,
        postmortems: Pm,
        rules: R,
        states: T,
        spans: Option<&'a Sp>,
    ) -> Self {
        Self {
            pages,
            incidents,
            timeline,
            postmortems,
            rules,
            states,
            spans,
        }
    }

    pub async fn view(&self, slug: &Slug, now: UnixSeconds) -> Result<PublicView, StatusError> {
        let page = self.public_page(slug).await?;
        self.render(&page, now).await
    }

    pub async fn preview<S, G, K>(
        &self,
        readers: &Readers<S, G, K>,
        who: &Reader,
        org: TenantId,
        id: StatusPageId,
        now: UnixSeconds,
    ) -> Result<PublicView, StatusError>
    where
        S: SessionStore,
        G: Memberships,
        K: ApiKeys,
    {
        readers.grant(who, org).await?;
        let page = self
            .pages
            .get(org, id)
            .await?
            .ok_or(StatusError::NoSuchPage)?;
        self.render(&page, now).await
    }

    pub async fn render(
        &self,
        page: &StatusPage,
        now: UnixSeconds,
    ) -> Result<PublicView, StatusError> {
        let components: Vec<_> = self
            .pages
            .components(page.org, page.id)
            .await?
            .into_iter()
            .filter(|component| component.public)
            .collect();
        let links: Vec<&Link> = components.iter().map(|component| &component.link).collect();
        let observed = self.observe(page, &links, now).await?;
        let shown_components: Vec<PublicComponent> = components
            .iter()
            .map(|component| PublicComponent {
                name: component.name.clone(),
                status: derive(&component.link, component.manual_status, &observed),
            })
            .collect();
        let incidents = self.incidents(page, now).await?;
        let open = incidents.iter().any(|incident| incident.status.is_open());
        Ok(PublicView {
            name: page.name.clone(),
            slug: page.slug.clone(),
            theme: page.theme,
            intro: page.intro.clone(),
            hide_from_search: page.hide_from_search,
            overall: overall(&shown_components, open),
            components: shown_components,
            incidents,
        })
    }

    pub async fn postmortem(
        &self,
        slug: &Slug,
        incident: IncidentId,
    ) -> Result<PublicPostmortem, StatusError> {
        let page = self.public_page(slug).await?;
        let held = self
            .incidents
            .get(page.org, incident)
            .await?
            .filter(|held| held.public)
            .ok_or(StatusError::NoSuchPostmortem)?;
        let postmortem = self
            .postmortems
            .of(page.org, incident)
            .await?
            .filter(|postmortem| postmortem.status == PostmortemStatus::Published)
            .ok_or(StatusError::NoSuchPostmortem)?;
        Ok(PublicPostmortem {
            title: held.title,
            body: redacted(postmortem.body.as_str()).into(),
            published_ms: postmortem.published_ms.unwrap_or(postmortem.updated_ms),
        })
    }

    async fn public_page(&self, slug: &Slug) -> Result<StatusPage, StatusError> {
        self.pages
            .by_slug(slug)
            .await?
            .filter(|page| page.public)
            .ok_or(StatusError::NoSuchPage)
    }

    async fn incidents(
        &self,
        page: &StatusPage,
        now: UnixSeconds,
    ) -> Result<Vec<PublicIncident>, StatusError> {
        let now_ms = now.get().saturating_mul(MILLIS_PER_SECOND);
        let held = self
            .incidents
            .list(
                page.org,
                &IncidentFilter {
                    scope: IncidentScope::All,
                    project: None,
                },
            )
            .await?;
        let mut shown_incidents = Vec::new();
        for incident in held.into_iter().filter(|held| shown(held, now_ms)) {
            let entries = self.timeline.of(page.org, incident.id).await?;
            let postmortem = self
                .postmortems
                .of(page.org, incident.id)
                .await?
                .is_some_and(|postmortem| postmortem.status == PostmortemStatus::Published);
            shown_incidents.push(PublicIncident {
                id: incident.id,
                title: incident.title,
                status: incident.status,
                severity: incident.severity,
                opened_ms: incident.opened_ms,
                resolved_ms: incident.resolved_ms,
                updates: public_updates(&entries),
                postmortem,
            });
        }
        Ok(shown_incidents)
    }

    async fn observe(
        &self,
        page: &StatusPage,
        links: &[&Link],
        now: UnixSeconds,
    ) -> Result<Observed, StatusError> {
        let mut observed = Observed::default();
        if links.iter().any(|link| matches!(link, Link::Alert(_))) {
            let rules = self.rules.list(page.org).await?;
            for (rule, series) in self.states.of_org(page.org).await? {
                if series.state.status != AlertStatus::Firing {
                    continue;
                }
                if let Some(held) = rules.iter().find(|held| held.id == rule) {
                    observed.firing.push((rule, held.definition.severity()));
                }
            }
        }
        let mut projects: Vec<Option<ProjectId>> = links
            .iter()
            .filter_map(|link| match link {
                Link::Service { project, .. } => Some(*project),
                Link::Alert(_) | Link::Manual => None,
            })
            .collect();
        projects.sort_unstable();
        projects.dedup();
        let (Some(spans), false) = (self.spans, projects.is_empty()) else {
            return Ok(observed);
        };
        let to = now.get().saturating_mul(NANOS_PER_SECOND);
        let from = to.saturating_sub(SERVICE_WINDOW_SECS * NANOS_PER_SECOND);
        let Ok(window) = Window::new(from, to) else {
            return Ok(observed);
        };
        for project in projects {
            let filter = ServiceFilter {
                window,
                project,
                environment: None,
            };
            let Ok(services) = spans.services(page.org, &filter).await else {
                continue;
            };
            observed
                .services
                .extend(services.into_iter().map(|summary| ServiceLoad {
                    name: summary.name,
                    project,
                    requests: summary.requests,
                    errors: summary.errors,
                }));
        }
        Ok(observed)
    }
}
