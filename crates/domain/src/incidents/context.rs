use core::cmp::Reverse;

use crate::explore::{SpanReader, TraceFilter, TraceOrder, TraceSummary, Window};
use crate::infrastructure::{InfraFilter, InfraReader, Resource};
use crate::issue::{Counted, IssueFilter, IssueReader};
use crate::releases::{Deploy, ReleaseFilter, ReleaseReader};
use crate::store::StoreError;

use super::entities::Incident;

pub const LOOKBACK_MS: u64 = 2 * 60 * 60 * 1_000;
pub const FIGURES_MS: u64 = 5 * 60 * 1_000;
pub const MAX_DEPLOYS: usize = 10;
pub const MAX_ISSUES: usize = 10;
pub const MAX_RESOURCES: usize = 10;
pub const MAX_TRACES: usize = 5;

const NANOS_PER_MILLI: u64 = 1_000_000;
const FETCH: u32 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LeftOut {
    pub deploys: usize,
    pub issues: usize,
    pub resources: usize,
    pub traces: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IncidentContext {
    pub services: Vec<Box<str>>,
    pub from_ms: u64,
    pub to_ms: u64,
    pub deploys: Vec<Deploy>,
    pub issues: Vec<Counted>,
    pub resources: Vec<Resource>,
    pub traces: Vec<TraceSummary>,
    pub left_out: LeftOut,
}

impl IncidentContext {
    pub fn empty(from_ms: u64, to_ms: u64) -> Self {
        Self {
            services: Vec::new(),
            from_ms,
            to_ms,
            deploys: Vec::new(),
            issues: Vec::new(),
            resources: Vec::new(),
            traces: Vec::new(),
            left_out: LeftOut::default(),
        }
    }
}

pub fn context_span_ms(opened_ms: u64, resolved_ms: Option<u64>, now_ms: u64) -> (u64, u64) {
    let from = opened_ms.saturating_sub(LOOKBACK_MS);
    let to = resolved_ms.unwrap_or(now_ms).max(from + 1);
    (from, to)
}

fn nanos(ms: u64) -> u64 {
    ms.saturating_mul(NANOS_PER_MILLI)
}

pub(crate) fn cap<T>(mut rows: Vec<T>, most: usize) -> (Vec<T>, usize) {
    let left = rows.len().saturating_sub(most);
    rows.truncate(most);
    (rows, left)
}

#[derive(Debug)]
pub struct Surroundings<'a, R, I, S, N> {
    releases: &'a R,
    issues: &'a I,
    spans: &'a S,
    infra: &'a N,
}

impl<'a, R, I, S, N> Surroundings<'a, R, I, S, N>
where
    R: ReleaseReader + Sync,
    I: IssueReader + Sync,
    S: SpanReader + Sync,
    N: InfraReader + Sync,
{
    pub fn new(releases: &'a R, issues: &'a I, spans: &'a S, infra: &'a N) -> Self {
        Self {
            releases,
            issues,
            spans,
            infra,
        }
    }

    pub async fn around(
        &self,
        incident: &Incident,
        services: &[Box<str>],
        now_ms: u64,
    ) -> Result<IncidentContext, StoreError> {
        let (from_ms, to_ms) = context_span_ms(incident.opened_ms, incident.resolved_ms, now_ms);
        let mut context = IncidentContext::empty(from_ms, to_ms);
        if services.is_empty() {
            return Ok(context);
        }
        let Ok(window) = Window::new(nanos(from_ms), nanos(to_ms)) else {
            return Ok(context);
        };
        context.services = services.to_vec();

        let (deploys, left) = cap(self.deploys(incident, services, window).await?, MAX_DEPLOYS);
        context.deploys = deploys;
        context.left_out.deploys = left;

        let (issues, left) = cap(
            self.new_issues(incident, services, window).await?,
            MAX_ISSUES,
        );
        context.issues = issues;
        context.left_out.issues = left;

        let (resources, left) = cap(
            self.resources(incident, services, to_ms).await?,
            MAX_RESOURCES,
        );
        context.resources = resources;
        context.left_out.resources = left;

        let (traces, left) = cap(self.failing(incident, services, window).await?, MAX_TRACES);
        context.traces = traces;
        context.left_out.traces = left;
        Ok(context)
    }

    async fn deploys(
        &self,
        incident: &Incident,
        services: &[Box<str>],
        window: Window,
    ) -> Result<Vec<Deploy>, StoreError> {
        let mut found = Vec::new();
        for service in services {
            let mut filter = ReleaseFilter::over(window);
            filter.service = Some(service.clone());
            filter.project = incident.project;
            found.extend(self.releases.deploys(incident.org, &filter).await?);
        }
        found.sort_by_key(|one| Reverse(one.at_unix_nanos));
        Ok(found)
    }

    async fn new_issues(
        &self,
        incident: &Incident,
        services: &[Box<str>],
        window: Window,
    ) -> Result<Vec<Counted>, StoreError> {
        let mut found = Vec::new();
        for service in services {
            let mut filter = IssueFilter::over(window).at_most(FETCH);
            filter.service = Some(service.clone());
            filter.project = incident.project;
            let rows = self.issues.issues(incident.org, &filter).await?;
            found.extend(
                rows.into_iter()
                    .filter(|issue| issue.first_seen_unix_nanos >= window.from_unix_nanos()),
            );
        }
        found.sort_by_key(|one| Reverse(one.first_seen_unix_nanos));
        Ok(found)
    }

    async fn resources(
        &self,
        incident: &Incident,
        services: &[Box<str>],
        to_ms: u64,
    ) -> Result<Vec<Resource>, StoreError> {
        let Ok(figures) = Window::new(nanos(to_ms.saturating_sub(FIGURES_MS)), nanos(to_ms) + 1)
        else {
            return Ok(Vec::new());
        };
        let filter = InfraFilter {
            project: incident.project,
            ..InfraFilter::default()
        };
        let mut found: Vec<Resource> = self
            .infra
            .resources(incident.org, figures, &filter)
            .await?
            .into_iter()
            .filter(|resource| {
                resource
                    .services
                    .iter()
                    .any(|served| services.contains(served))
            })
            .map(|resource| resource.settled(nanos(to_ms)))
            .collect();
        found.sort_by(|one, other| one.name.cmp(&other.name));
        Ok(found)
    }

    async fn failing(
        &self,
        incident: &Incident,
        services: &[Box<str>],
        window: Window,
    ) -> Result<Vec<TraceSummary>, StoreError> {
        let mut found = Vec::new();
        for service in services {
            let mut filter =
                TraceFilter::over(window).at_most(u32::try_from(MAX_TRACES + 1).unwrap_or(6));
            filter.service = Some(service.clone());
            filter.project = incident.project;
            filter.only_errors = true;
            filter.order = TraceOrder::Duration;
            found.extend(self.spans.traces(incident.org, &filter).await?);
        }
        found.sort_by_key(|one| Reverse(one.duration_nanos));
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::{LOOKBACK_MS, cap, context_span_ms};

    #[test]
    fn an_open_incident_is_read_from_two_hours_before_it_opened_until_now() {
        let opened = 10 * LOOKBACK_MS;
        let now = opened + 5_000;

        assert_eq!(
            context_span_ms(opened, None, now),
            (opened - LOOKBACK_MS, now)
        );
    }

    #[test]
    fn a_resolved_incident_stops_at_its_resolution() {
        let opened = 10 * LOOKBACK_MS;
        let resolved = opened + 1_000;

        assert_eq!(
            context_span_ms(opened, Some(resolved), resolved + 99_000),
            (opened - LOOKBACK_MS, resolved)
        );
    }

    #[test]
    fn an_incident_that_opened_at_the_epoch_does_not_underflow() {
        let (from, to) = context_span_ms(0, None, 0);

        assert_eq!(from, 0);
        assert!(to > from);
    }

    #[test]
    fn a_list_over_the_cap_is_cut_and_the_rest_counted() {
        let (kept, left) = cap(vec![1, 2, 3, 4, 5], 3);

        assert_eq!(kept, vec![1, 2, 3]);
        assert_eq!(left, 2);
    }

    #[test]
    fn a_list_under_the_cap_is_kept_whole() {
        let (kept, left) = cap(vec![1, 2], 3);

        assert_eq!(kept, vec![1, 2]);
        assert_eq!(left, 0);
    }
}
