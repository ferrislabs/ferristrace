use super::entities::{Link, Observed, PublicComponent, PublicUpdate};
use super::value_objects::{
    ComponentStatus, DEGRADED_ERROR_PERCENT, DOWN_ERROR_PERCENT, PUBLIC_WINDOW_DAYS, UpdateKind,
};
use crate::alerts::AlertSeverity;
use crate::incidents::{Change, Incident, IncidentStatus, TimelineEntry};

const DAY_MS: u64 = 86_400_000;
const PERCENT: u128 = 100;
const REDACTED: &str = "[redacted]";

pub fn derive(
    link: &Link,
    manual: Option<ComponentStatus>,
    observed: &Observed,
) -> ComponentStatus {
    match link {
        Link::Manual => manual.unwrap_or_default(),
        Link::Alert(rule) => match observed.firing(*rule) {
            Some(AlertSeverity::Critical) => ComponentStatus::Down,
            Some(AlertSeverity::Warning | AlertSeverity::Info) => ComponentStatus::Degraded,
            None => ComponentStatus::Operational,
        },
        Link::Service { name, project } => observed
            .load(name, *project)
            .map_or(ComponentStatus::Operational, |load| {
                by_error_rate(load.requests, load.errors)
            }),
    }
}

pub fn by_error_rate(requests: u64, errors: u64) -> ComponentStatus {
    if requests == 0 {
        return ComponentStatus::Operational;
    }
    let failed = u128::from(errors) * PERCENT;
    let seen = u128::from(requests);
    if failed >= seen * u128::from(DOWN_ERROR_PERCENT) {
        ComponentStatus::Down
    } else if failed >= seen * u128::from(DEGRADED_ERROR_PERCENT) {
        ComponentStatus::Degraded
    } else {
        ComponentStatus::Operational
    }
}

pub fn overall(components: &[PublicComponent], incident_open: bool) -> ComponentStatus {
    let worst = components
        .iter()
        .map(|component| component.status)
        .max()
        .unwrap_or_default();
    if incident_open {
        worst.max(ComponentStatus::Degraded)
    } else {
        worst
    }
}

pub fn shown(incident: &Incident, now_ms: u64) -> bool {
    incident.public
        && (incident.status.is_open()
            || incident.resolved_ms.is_some_and(|resolved| {
                resolved.saturating_add(PUBLIC_WINDOW_DAYS * DAY_MS) >= now_ms
            }))
}

pub fn public_updates(entries: &[TimelineEntry]) -> Vec<PublicUpdate> {
    let mut updates: Vec<PublicUpdate> = entries
        .iter()
        .filter_map(|entry| {
            let (kind, text): (UpdateKind, Box<str>) = match &entry.change {
                Change::Opened { .. } => (
                    UpdateKind::Opened,
                    IncidentStatus::Investigating.as_str().into(),
                ),
                Change::Status { to, .. } => (UpdateKind::Status, to.as_str().into()),
                Change::Comment { body, public: true } => {
                    (UpdateKind::Comment, body.as_str().into())
                }
                _ => return None,
            };
            Some(PublicUpdate {
                at_ms: entry.at_ms,
                kind,
                text,
            })
        })
        .collect();
    updates.reverse();
    updates
}

fn local(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'%' | b'+' | b'-')
}

fn host(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-')
}

pub fn redacted(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut at = 0;
    while let Some(found) = text[at..].find('@') {
        let sign = at + found;
        let start = (copied..sign)
            .rev()
            .take_while(|&index| local(bytes[index]))
            .last()
            .unwrap_or(sign);
        let mut end = sign
            + 1
            + bytes[sign + 1..]
                .iter()
                .take_while(|&&byte| host(byte))
                .count();
        while end > sign + 1 && matches!(bytes[end - 1], b'.' | b'-') {
            end -= 1;
        }
        let domain = &text[sign + 1..end];
        if start < sign && domain.contains('.') && !domain.starts_with('.') {
            out.push_str(&text[copied..start]);
            out.push_str(REDACTED);
            copied = end;
            at = end;
        } else {
            at = sign + 1;
        }
    }
    out.push_str(&text[copied..]);
    out
}
