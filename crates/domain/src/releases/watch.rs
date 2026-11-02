use super::entities::Deployed;
use super::errors::ReleaseError;
use super::ports::{DeployFeed, DeployMarks};
use super::value_objects::{DEPLOY_WATCH_TICK_SECS, LATE_SECS};
use crate::alerts::{AlertDuty, AlertRules, Duty, Signal};
use crate::clock::UnixSeconds;
use crate::explore::Window;
use crate::incidents::{
    Author, Change, Incident, IncidentFilter, IncidentScope, IncidentTimeline, Incidents,
    TimelineEntry, TimelineId,
};
use crate::telemetry::TenantId;

const MILLIS_PER_SECOND: u64 = 1_000;
const NANOS_PER_MILLI: u64 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Followed {
    Skipped,
    Worked { deploys: usize, entries: usize },
}

#[derive(Debug)]
pub struct Watch<'a, F, I, L, A, M, D> {
    feed: &'a F,
    incidents: I,
    timeline: L,
    rules: A,
    marks: M,
    duty: D,
}

fn services_named(signal: &Signal) -> Option<Vec<&str>> {
    match signal {
        Signal::Traces(traces) => traces.service.as_deref().map(|service| vec![service]),
        Signal::Logs(logs) if !logs.services.is_empty() => {
            Some(logs.services.iter().map(AsRef::as_ref).collect())
        }
        Signal::Logs(_) | Signal::Metrics(_) => None,
    }
}

impl<'a, F, I, L, A, M, D> Watch<'a, F, I, L, A, M, D>
where
    F: DeployFeed,
    I: Incidents,
    L: IncidentTimeline,
    A: AlertRules,
    M: DeployMarks,
    D: AlertDuty,
{
    pub fn new(feed: &'a F, incidents: I, timeline: L, rules: A, marks: M, duty: D) -> Self {
        Self {
            feed,
            incidents,
            timeline,
            rules,
            marks,
            duty,
        }
    }

    pub async fn tick(&self, now: UnixSeconds) -> Result<Followed, ReleaseError> {
        if !self
            .duty
            .lead(Duty::Deploys, now.get() / DEPLOY_WATCH_TICK_SECS)
            .await?
        {
            return Ok(Followed::Skipped);
        }
        let to_ms = now.get().saturating_mul(MILLIS_PER_SECOND);
        let Some(mark) = self.marks.watermark().await? else {
            self.marks.advance(to_ms).await?;
            return Ok(Followed::Worked {
                deploys: 0,
                entries: 0,
            });
        };
        let from_ms = mark.saturating_sub(LATE_SECS * MILLIS_PER_SECOND);
        let Ok(window) = Window::new(
            from_ms.saturating_mul(NANOS_PER_MILLI),
            to_ms.saturating_mul(NANOS_PER_MILLI),
        ) else {
            return Ok(Followed::Worked {
                deploys: 0,
                entries: 0,
            });
        };
        let deployed = self.feed.deployed(window).await?;
        let mut orgs: Vec<TenantId> = Vec::new();
        for held in &deployed {
            if !orgs.contains(&held.org) {
                orgs.push(held.org);
            }
        }
        let mut entries = 0;
        for org in orgs {
            entries += self.follow(org, &deployed).await?;
        }
        self.marks.advance(to_ms).await?;
        Ok(Followed::Worked {
            deploys: deployed.len(),
            entries,
        })
    }

    async fn follow(&self, org: TenantId, deployed: &[Deployed]) -> Result<usize, ReleaseError> {
        let open = self
            .incidents
            .list(
                org,
                &IncidentFilter {
                    scope: IncidentScope::Open,
                    project: None,
                },
            )
            .await?;
        let mut entries = 0;
        for incident in &open {
            let named = self.named_by(incident).await?;
            for held in deployed.iter().filter(|held| held.org == org) {
                let deploy = &held.deploy;
                let relevant = deploy.at_ms() >= incident.opened_ms
                    && named.as_ref().is_none_or(|services| {
                        services.iter().any(|service| **service == *deploy.service)
                    });
                if !relevant {
                    continue;
                }
                let entry = TimelineEntry {
                    id: TimelineId::fresh(),
                    incident: incident.id,
                    org,
                    author: Author::System,
                    change: Change::Deploy {
                        service: deploy.service.clone(),
                        version: deploy.version.clone(),
                        environment: deploy.environment.clone(),
                    },
                    at_ms: deploy.at_ms(),
                };
                if self.timeline.append_once(&entry).await? {
                    entries += 1;
                }
            }
        }
        Ok(entries)
    }

    async fn named_by(&self, incident: &Incident) -> Result<Option<Vec<Box<str>>>, ReleaseError> {
        let Some(rule) = incident.opened_from_alert else {
            return Ok(None);
        };
        Ok(self.rules.get(incident.org, rule).await?.and_then(|rule| {
            services_named(&rule.definition.parts().signal)
                .map(|names| names.into_iter().map(Box::from).collect())
        }))
    }
}
