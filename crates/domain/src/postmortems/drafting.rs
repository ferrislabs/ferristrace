use jiff::Timestamp;

use crate::alerts::{AlertStatus, RuleId};
use crate::incidents::{
    Attachment, AttachmentKind, Change, Chronicle, FORMER_MEMBER, Incident, Person, TimelineEntry,
};

const MILLIS_PER_MINUTE: u64 = 60_000;
const SUMMARY_PLACEHOLDER: &str = "_Write what happened in two sentences._";

pub fn draft(incident: &Incident, chronicle: &Chronicle, attachments: &[Attachment]) -> String {
    let mut lines = vec![
        format!("# Postmortem: {}", incident.title),
        String::new(),
        "## Summary".to_owned(),
        String::new(),
        SUMMARY_PLACEHOLDER.to_owned(),
        String::new(),
        "## Impact".to_owned(),
        String::new(),
        format!("- Window: {}", window(incident)),
        format!("- Severity: {}", incident.severity),
        format!("- Owner: {}", owner(incident, chronicle)),
        String::new(),
        "## Timeline".to_owned(),
        String::new(),
        "| Time (UTC) | Author | What happened |".to_owned(),
        "| --- | --- | --- |".to_owned(),
    ];
    lines.extend(chronicle.entries.iter().map(|entry| row(chronicle, entry)));
    lines.extend(section(
        "Contributing alerts",
        alerts(incident, chronicle),
        "_No alert contributed._",
    ));
    lines.extend(section("Paging", paging(chronicle), "_Nobody was paged._"));
    lines.extend(section(
        "Attachments",
        attachments.iter().map(link).collect(),
        "_Nothing was attached._",
    ));
    lines.extend([
        String::new(),
        "## Action items".to_owned(),
        String::new(),
        "- [ ] ".to_owned(),
    ]);
    lines.join("\n") + "\n"
}

fn section(title: &str, items: Vec<String>, empty: &str) -> Vec<String> {
    let mut lines = vec![String::new(), format!("## {title}"), String::new()];
    if items.is_empty() {
        lines.push(empty.to_owned());
    } else {
        lines.extend(items);
    }
    lines
}

fn stamp(ms: u64) -> String {
    i64::try_from(ms)
        .ok()
        .and_then(|ms| Timestamp::from_millisecond(ms).ok())
        .map_or_else(
            || "an unknown time".to_owned(),
            |at| format!("{} UTC", at.strftime("%Y-%m-%d %H:%M:%S")),
        )
}

fn window(incident: &Incident) -> String {
    let opened = stamp(incident.opened_ms);
    match incident.resolved_ms {
        None => format!("{opened} \u{2192} still open"),
        Some(resolved) => {
            let minutes = resolved.saturating_sub(incident.opened_ms) / MILLIS_PER_MINUTE;
            let unit = if minutes == 1 { "minute" } else { "minutes" };
            format!("{opened} \u{2192} {} ({minutes} {unit})", stamp(resolved))
        }
    }
}

fn owner(incident: &Incident, chronicle: &Chronicle) -> String {
    incident.owner.map_or_else(
        || "nobody".to_owned(),
        |owner| person(chronicle, owner).to_owned(),
    )
}

fn person(chronicle: &Chronicle, account: crate::identity::AccountId) -> &str {
    chronicle
        .person(account)
        .map_or(FORMER_MEMBER, Person::label)
}

fn maybe_person(chronicle: &Chronicle, account: Option<crate::identity::AccountId>) -> &str {
    account.map_or("nobody", |account| person(chronicle, account))
}

fn alert_name(chronicle: &Chronicle, rule: RuleId) -> &str {
    chronicle
        .alert(rule)
        .map_or("a deleted alert", |alert| &alert.name)
}

fn cell(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('|', "\\|")
}

fn spoken(status: AlertStatus) -> String {
    status.as_str().replace('_', " ")
}

fn row(chronicle: &Chronicle, entry: &TimelineEntry) -> String {
    format!(
        "| {} | {} | {} |",
        stamp(entry.at_ms),
        cell(chronicle.author_label(&entry.author)),
        cell(&told(chronicle, &entry.change)),
    )
}

fn told(chronicle: &Chronicle, change: &Change) -> String {
    match change {
        Change::Opened {
            title,
            severity,
            alert,
        } => {
            let from = alert.map_or_else(String::new, |rule| {
                format!(" from the alert {}", alert_name(chronicle, rule))
            });
            format!("opened \"{title}\" as {severity}{from}")
        }
        Change::Status { from, to } => format!("status {from} \u{2192} {to}"),
        Change::Severity { from, to } => format!("severity {from} \u{2192} {to}"),
        Change::Owner { from, to } => format!(
            "owner {} \u{2192} {}",
            maybe_person(chronicle, *from),
            maybe_person(chronicle, *to)
        ),
        Change::Title { from, to } => format!("renamed \"{from}\" \u{2192} \"{to}\""),
        Change::Public { to, .. } => {
            if *to {
                "made the incident public".to_owned()
            } else {
                "made the incident private".to_owned()
            }
        }
        Change::Comment { body } => format!("commented: \"{}\"", body.as_str()),
        Change::Alert {
            rule,
            from,
            to,
            value,
            ..
        } => {
            let seen = value.map_or_else(String::new, |value| format!(" (value {value})"));
            format!(
                "alert {} {} \u{2192} {}{seen}",
                alert_name(chronicle, *rule),
                spoken(*from),
                spoken(*to)
            )
        }
        Change::Attachment(attachment) => format!(
            "attached {} \"{}\"",
            attachment.kind(),
            attachment.label.as_str()
        ),
        Change::Paged { step, target, .. } => format!(
            "paged {} (step {step})",
            target.map_or("every administrator", |target| person(chronicle, target))
        ),
        Change::Acknowledged { .. } => "acknowledged the page".to_owned(),
    }
}

fn alerts(incident: &Incident, chronicle: &Chronicle) -> Vec<String> {
    let mut rules: Vec<RuleId> = incident.opened_from_alert.into_iter().collect();
    for entry in &chronicle.entries {
        if let Change::Alert { rule, .. } = &entry.change
            && !rules.contains(rule)
        {
            rules.push(*rule);
        }
    }
    rules
        .into_iter()
        .map(|rule| {
            let first = chronicle.entries.iter().find(|entry| {
                matches!(&entry.change,
                    Change::Alert { rule: held, to: AlertStatus::Firing, .. } if *held == rule)
            });
            let name = alert_name(chronicle, rule);
            match first {
                Some(entry) => format!("- {name}, first firing {}", stamp(entry.at_ms)),
                None => format!(
                    "- {name}, opened the incident {}",
                    stamp(incident.opened_ms)
                ),
            }
        })
        .collect()
}

fn paging(chronicle: &Chronicle) -> Vec<String> {
    let entries = &chronicle.entries;
    entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| match &entry.change {
            Change::Paged { page, step, target } => Some((index, entry, *page, *step, *target)),
            _ => None,
        })
        .map(|(index, entry, page, step, target)| {
            let who = target.map_or("every administrator", |target| person(chronicle, target));
            let later = entries[index + 1..].iter().any(
                |later| matches!(&later.change, Change::Paged { page: held, .. } if *held == page),
            );
            let acknowledged = entries.iter().find(
                |held| matches!(&held.change, Change::Acknowledged { page: ack } if *ack == page),
            );
            let outcome = match (later, acknowledged) {
                (true, _) => "escalated".to_owned(),
                (false, Some(ack)) => format!(
                    "acknowledged {} by {}",
                    stamp(ack.at_ms),
                    chronicle.author_label(&ack.author)
                ),
                (false, None) => "not acknowledged".to_owned(),
            };
            format!(
                "- {who} paged {} (step {step}), {outcome}",
                stamp(entry.at_ms)
            )
        })
        .collect()
}

fn encoded(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(byte).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

fn link(attachment: &Attachment) -> String {
    let label = cell(attachment.label.as_str())
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]");
    let reference = attachment.reference.as_str();
    let target = match attachment.kind() {
        AttachmentKind::Trace => format!("/traces/{reference}"),
        AttachmentKind::Logs => format!("/logs?f={}", encoded(reference)),
        AttachmentKind::Metric => format!("/metrics?f={}", encoded(reference)),
    };
    format!("- [{label}]({target})")
}
