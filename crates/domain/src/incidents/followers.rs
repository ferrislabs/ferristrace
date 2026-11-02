use super::entities::{Author, Change, TimelineEntry};
use super::ports::{IncidentTimeline, Incidents};
use super::value_objects::TimelineId;
use crate::alerts::{AlertEvent, AlertStatus};
use crate::store::StoreError;

#[derive(Debug)]
pub struct Followers<I, L> {
    incidents: I,
    timeline: L,
}

impl<I, L> Followers<I, L>
where
    I: Incidents,
    L: IncidentTimeline,
{
    pub fn new(incidents: I, timeline: L) -> Self {
        Self {
            incidents,
            timeline,
        }
    }

    pub async fn tell(&self, event: &AlertEvent) -> Result<usize, StoreError> {
        let crosses_firing = event.from != event.to
            && (event.from == AlertStatus::Firing || event.to == AlertStatus::Firing);
        if !crosses_firing {
            return Ok(0);
        }
        let following = self.incidents.following(event.org, event.rule).await?;
        for incident in &following {
            self.timeline
                .append(&TimelineEntry {
                    id: TimelineId::fresh(),
                    incident: *incident,
                    org: event.org,
                    author: Author::System,
                    change: Change::Alert {
                        rule: event.rule,
                        from: event.from,
                        to: event.to,
                        value: event.value,
                        labels: event.labels.clone(),
                    },
                    at_ms: event.at_ms,
                })
                .await?;
        }
        Ok(following.len())
    }
}
