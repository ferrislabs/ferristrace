use ferristrace_domain::{
    AccountId, AgentId, AlertStatus, Attachment, AttachmentId, AttachmentKind, AttachmentLabel,
    Author, Change, CommentBody, EntryKind, IncidentId, IncidentSeverity, IncidentStatus,
    IncidentTitle, KeyId, PageId, Reference, RuleId,
};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::alert_codec::{culprits_from, culprits_value, encode_labels, labels_of};

fn id_of(value: Option<Uuid>) -> Value {
    value.map_or(Value::Null, |id| Value::String(id.to_string()))
}

pub(crate) fn encode_change(change: &Change) -> (Option<&str>, String) {
    let detail = match change {
        Change::Opened {
            title,
            severity,
            alert,
        } => json!({
            "title": title.as_str(),
            "severity": severity.as_str(),
            "alert": id_of(alert.map(RuleId::as_uuid)),
        }),
        Change::Status { from, to } => json!({"from": from.as_str(), "to": to.as_str()}),
        Change::Severity { from, to } => json!({"from": from.as_str(), "to": to.as_str()}),
        Change::Owner { from, to } => json!({
            "from": id_of(from.map(AccountId::as_uuid)),
            "to": id_of(to.map(AccountId::as_uuid)),
        }),
        Change::Title { from, to } => json!({"from": from.as_str(), "to": to.as_str()}),
        Change::Public { from, to } => json!({"from": from, "to": to}),
        Change::Comment { public, .. } => json!({"public": public}),
        Change::Alert {
            rule,
            from,
            to,
            value,
            labels,
            context,
        } => json!({
            "rule": rule.to_string(),
            "from": from.as_str(),
            "to": to.as_str(),
            "value": value,
            "labels": encode_labels(labels),
            "context": context.as_ref().map_or(Value::Null, culprits_value),
        }),
        Change::Attachment(attachment) => json!({
            "id": attachment.id.to_string(),
            "kind": attachment.kind().as_str(),
            "reference": attachment.reference.as_str(),
            "label": attachment.label.as_str(),
            "at_ms": attachment.at_ms,
        }),
        Change::Paged { page, step, target } => json!({
            "page": page.to_string(),
            "step": step,
            "target": id_of(target.map(AccountId::as_uuid)),
        }),
        Change::Acknowledged { page } => json!({"page": page.to_string()}),
        Change::Deploy {
            service,
            version,
            environment,
        } => json!({
            "service": service,
            "version": version,
            "environment": environment,
        }),
    };
    let body = match change {
        Change::Comment { body, .. } => Some(body.as_str()),
        _ => None,
    };
    (body, detail.to_string())
}

fn text<'a>(detail: &'a Value, key: &str) -> Option<&'a str> {
    detail.get(key)?.as_str()
}

struct Unreadable;

fn uuid_at(detail: &Value, key: &str) -> Result<Option<Uuid>, Unreadable> {
    match detail.get(key) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(held)) => Uuid::try_parse(held).map(Some).map_err(|_| Unreadable),
        _ => Err(Unreadable),
    }
}

fn pair<T>(detail: &Value, parse: impl Fn(&str) -> Option<T>) -> Option<(T, T)> {
    Some((parse(text(detail, "from")?)?, parse(text(detail, "to")?)?))
}

pub(crate) fn decode_change(
    kind: &str,
    body: Option<&str>,
    detail: &str,
    incident: IncidentId,
) -> Option<Change> {
    let detail: Value = serde_json::from_str(detail).ok()?;
    Some(match EntryKind::parse(kind)? {
        EntryKind::Opened => Change::Opened {
            title: IncidentTitle::try_from(text(&detail, "title")?).ok()?,
            severity: IncidentSeverity::parse(text(&detail, "severity")?)?,
            alert: uuid_at(&detail, "alert").ok()?.map(RuleId::new),
        },
        EntryKind::Status => {
            let (from, to) = pair(&detail, IncidentStatus::parse)?;
            Change::Status { from, to }
        }
        EntryKind::Severity => {
            let (from, to) = pair(&detail, IncidentSeverity::parse)?;
            Change::Severity { from, to }
        }
        EntryKind::Owner => Change::Owner {
            from: uuid_at(&detail, "from").ok()?.map(AccountId::new),
            to: uuid_at(&detail, "to").ok()?.map(AccountId::new),
        },
        EntryKind::Title => {
            let (from, to) = pair(&detail, |held| IncidentTitle::try_from(held).ok())?;
            Change::Title { from, to }
        }
        EntryKind::Public => Change::Public {
            from: detail.get("from")?.as_bool()?,
            to: detail.get("to")?.as_bool()?,
        },
        EntryKind::Comment => Change::Comment {
            body: CommentBody::try_from(body?).ok()?,
            public: detail
                .get("public")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        },
        EntryKind::Alert => {
            let (from, to) = pair(&detail, AlertStatus::parse)?;
            Change::Alert {
                rule: RuleId::new(uuid_at(&detail, "rule").ok()??),
                from,
                to,
                value: detail.get("value").and_then(Value::as_f64),
                labels: labels_of(text(&detail, "labels")?)?,
                context: detail.get("context").and_then(culprits_from),
            }
        }
        EntryKind::Attachment => {
            let kind = AttachmentKind::parse(text(&detail, "kind")?)?;
            Change::Attachment(Attachment {
                id: AttachmentId::new(uuid_at(&detail, "id").ok()??),
                incident,
                reference: Reference::parse(kind, text(&detail, "reference")?).ok()?,
                label: AttachmentLabel::try_from(text(&detail, "label")?).ok()?,
                at_ms: detail.get("at_ms")?.as_u64()?,
            })
        }
        EntryKind::Paged => Change::Paged {
            page: PageId::new(uuid_at(&detail, "page").ok()??),
            step: u32::try_from(detail.get("step")?.as_u64()?).ok()?,
            target: uuid_at(&detail, "target").ok()?.map(AccountId::new),
        },
        EntryKind::Acknowledged => Change::Acknowledged {
            page: PageId::new(uuid_at(&detail, "page").ok()??),
        },
        EntryKind::Deploy => Change::Deploy {
            service: text(&detail, "service")?.into(),
            version: text(&detail, "version")?.into(),
            environment: text(&detail, "environment")?.into(),
        },
    })
}

pub(crate) fn author_parts(author: &Author) -> (&'static str, Option<Uuid>, Option<&str>) {
    match author {
        Author::Account(account) => ("account", Some(account.as_uuid()), None),
        Author::ApiKey(key) => ("api_key", Some(key.as_uuid()), None),
        Author::Agent { id, name } => ("agent", id.map(AgentId::as_uuid), Some(name)),
        Author::System => ("system", None, None),
    }
}

pub(crate) fn author_of(kind: &str, id: Option<Uuid>, label: Option<&str>) -> Option<Author> {
    match (kind, id, label) {
        ("account", Some(id), _) => Some(Author::Account(AccountId::new(id))),
        ("api_key", Some(id), _) => Some(Author::ApiKey(KeyId::new(id))),
        ("agent", id, Some(name)) => Some(Author::Agent {
            id: id.map(AgentId::new),
            name: name.into(),
        }),
        ("system", None, _) => Some(Author::System),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ferristrace_domain::{
        AccountId, AgentId, AlertStatus, Attachment, AttachmentId, AttachmentKind, AttachmentLabel,
        Author, Change, CommentBody, CulpritPod, Culprits, IncidentId, IncidentSeverity,
        IncidentStatus, IncidentTitle, KeyId, Labels, PageId, Reference, RuleId,
    };
    use uuid::Uuid;

    use super::{author_of, author_parts, decode_change, encode_change};

    fn blamed() -> Culprits {
        Culprits::new(
            vec![CulpritPod {
                name: "mail-1".into(),
                node: Some("node-a".into()),
                count: 4,
            }],
            6,
            vec!["node-a".into()],
            vec!["2.0.1".into()],
            &Labels::none(),
        )
    }

    #[test]
    fn every_change_reads_back_from_what_is_stored() {
        let incident = IncidentId::new(Uuid::from_u128(1));
        let title = |text: &str| IncidentTitle::try_from(text).expect("a title");
        let changes = vec![
            Change::Opened {
                title: title("Checkout down"),
                severity: IncidentSeverity::Sev2,
                alert: Some(RuleId::new(Uuid::from_u128(2))),
            },
            Change::Opened {
                title: title("By hand"),
                severity: IncidentSeverity::Sev4,
                alert: None,
            },
            Change::Status {
                from: IncidentStatus::Investigating,
                to: IncidentStatus::Resolved,
            },
            Change::Severity {
                from: IncidentSeverity::Sev3,
                to: IncidentSeverity::Sev1,
            },
            Change::Owner {
                from: None,
                to: Some(AccountId::new(Uuid::from_u128(3))),
            },
            Change::Title {
                from: title("One"),
                to: title("Two"),
            },
            Change::Public {
                from: false,
                to: true,
            },
            Change::Comment {
                body: CommentBody::try_from("rolled back").expect("a body"),
                public: true,
            },
            Change::Comment {
                body: CommentBody::try_from("the db is full").expect("a body"),
                public: false,
            },
            Change::Alert {
                rule: RuleId::new(Uuid::from_u128(2)),
                from: AlertStatus::Firing,
                to: AlertStatus::Ok,
                value: Some(1.5),
                labels: Labels::new(BTreeMap::from([("queue".into(), "mail".into())])),
                context: Some(blamed()),
            },
            Change::Alert {
                rule: RuleId::new(Uuid::from_u128(2)),
                from: AlertStatus::Pending,
                to: AlertStatus::Firing,
                value: None,
                labels: Labels::none(),
                context: None,
            },
            Change::Attachment(Attachment {
                id: AttachmentId::new(Uuid::from_u128(4)),
                incident,
                reference: Reference::parse(AttachmentKind::Metric, "metric=queue.depth")
                    .expect("a query"),
                label: AttachmentLabel::try_from("depth").expect("a label"),
                at_ms: 42,
            }),
            Change::Paged {
                page: PageId::new(Uuid::from_u128(5)),
                step: 2,
                target: Some(AccountId::new(Uuid::from_u128(6))),
            },
            Change::Paged {
                page: PageId::new(Uuid::from_u128(5)),
                step: 3,
                target: None,
            },
            Change::Acknowledged {
                page: PageId::new(Uuid::from_u128(5)),
            },
            Change::Deploy {
                service: "checkout".into(),
                version: "2.4.1".into(),
                environment: "prod".into(),
            },
            Change::Deploy {
                service: "mailer".into(),
                version: "7".into(),
                environment: "".into(),
            },
        ];

        for change in changes {
            let (body, detail) = encode_change(&change);
            assert_eq!(
                decode_change(change.kind().as_str(), body, &detail, incident),
                Some(change)
            );
        }
    }

    #[test]
    fn every_author_reads_back() {
        for author in [
            Author::Account(AccountId::new(Uuid::from_u128(1))),
            Author::ApiKey(KeyId::new(Uuid::from_u128(2))),
            Author::Agent {
                id: Some(AgentId::new(Uuid::from_u128(3))),
                name: "triage".into(),
            },
            Author::Agent {
                id: None,
                name: "legacy".into(),
            },
            Author::System,
        ] {
            let (kind, id, label) = author_parts(&author);
            assert_eq!(author_of(kind, id, label), Some(author));
        }
    }
}
