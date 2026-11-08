use std::collections::{BTreeMap, HashMap};

use super::entities::Incident;
use super::value_objects::{IncidentId, IncidentSeverity};
use crate::alerts::RuleId;

pub const DEFAULT_WINDOW_DAYS: u64 = 30;
pub const MAX_WINDOW_DAYS: u64 = 366;

const DAY_MS: u64 = 86_400_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Timing {
    pub samples: usize,
    pub mean_ms: Option<u64>,
    pub median_ms: Option<u64>,
}

impl Timing {
    pub(crate) fn of(mut spans: Vec<u64>) -> Self {
        if spans.is_empty() {
            return Self::default();
        }
        spans.sort_unstable();
        let samples = spans.len();
        let total: u128 = spans.iter().map(|span| u128::from(*span)).sum();
        let mean = u64::try_from(total / samples as u128).ok();
        let middle = samples / 2;
        let median = if samples % 2 == 1 {
            spans[middle]
        } else {
            spans[middle - 1].midpoint(spans[middle])
        };
        Self {
            samples,
            mean_ms: mean,
            median_ms: Some(median),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Group {
    pub incidents: usize,
    pub open: usize,
    pub resolved: usize,
    pub acknowledge: Timing,
    pub resolve: Timing,
}

impl Group {
    fn of(members: &[&Incident], responses: &HashMap<IncidentId, u64>) -> Self {
        let acknowledged = members
            .iter()
            .filter_map(|incident| {
                responses
                    .get(&incident.id)
                    .map(|at| at.saturating_sub(incident.opened_ms))
            })
            .collect();
        let resolved: Vec<u64> = members
            .iter()
            .filter_map(|incident| {
                incident
                    .resolved_ms
                    .map(|at| at.saturating_sub(incident.opened_ms))
            })
            .collect();
        Self {
            incidents: members.len(),
            open: members.len() - resolved.len(),
            resolved: resolved.len(),
            acknowledge: Timing::of(acknowledged),
            resolve: Timing::of(resolved),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Insights {
    pub from_ms: u64,
    pub to_ms: u64,
    pub overall: Group,
    pub by_severity: Vec<(IncidentSeverity, Group)>,
    pub by_service: Vec<(Option<Box<str>>, Group)>,
    pub per_day: Vec<(u64, usize)>,
}

const SEVERITIES: [IncidentSeverity; 4] = [
    IncidentSeverity::Sev1,
    IncidentSeverity::Sev2,
    IncidentSeverity::Sev3,
    IncidentSeverity::Sev4,
];

impl Insights {
    pub fn compute(
        incidents: &[Incident],
        responses: &HashMap<IncidentId, u64>,
        services: &HashMap<RuleId, Vec<Box<str>>>,
        from_ms: u64,
        to_ms: u64,
    ) -> Self {
        let inside: Vec<&Incident> = incidents
            .iter()
            .filter(|incident| incident.opened_ms >= from_ms && incident.opened_ms < to_ms)
            .collect();

        let by_severity = SEVERITIES
            .into_iter()
            .filter_map(|severity| {
                let members: Vec<&Incident> = inside
                    .iter()
                    .copied()
                    .filter(|incident| incident.severity == severity)
                    .collect();
                (!members.is_empty()).then(|| (severity, Group::of(&members, responses)))
            })
            .collect();

        let mut grouped: BTreeMap<Option<Box<str>>, Vec<&Incident>> = BTreeMap::new();
        for incident in &inside {
            let served = incident
                .opened_from_alert
                .and_then(|rule| services.get(&rule))
                .filter(|names| !names.is_empty());
            match served {
                Some(names) => {
                    for name in names {
                        grouped
                            .entry(Some(name.clone()))
                            .or_default()
                            .push(incident);
                    }
                }
                None => grouped.entry(None).or_default().push(incident),
            }
        }
        let mut by_service: Vec<(Option<Box<str>>, Group)> = grouped
            .into_iter()
            .map(|(name, members)| (name, Group::of(&members, responses)))
            .collect();
        by_service.sort_by(|one, other| {
            other
                .1
                .incidents
                .cmp(&one.1.incidents)
                .then_with(|| one.0.cmp(&other.0))
        });

        Self {
            from_ms,
            to_ms,
            overall: Group::of(&inside, responses),
            by_severity,
            by_service,
            per_day: per_day(&inside, from_ms, to_ms),
        }
    }
}

fn per_day(incidents: &[&Incident], from_ms: u64, to_ms: u64) -> Vec<(u64, usize)> {
    let first = from_ms / DAY_MS * DAY_MS;
    let mut days: Vec<(u64, usize)> = Vec::new();
    let most = usize::try_from(MAX_WINDOW_DAYS + 1).unwrap_or(usize::MAX);
    let mut day = first;
    while day < to_ms && days.len() < most {
        days.push((day, 0));
        day += DAY_MS;
    }
    for incident in incidents {
        let at = incident.opened_ms / DAY_MS * DAY_MS;
        if let Some(slot) = days.iter_mut().find(|(start, _)| *start == at) {
            slot.1 += 1;
        }
    }
    days
}
