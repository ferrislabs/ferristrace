use ferristrace_domain::{AlertSeverity, AlertStatus, Dispatch};
use serde_json::{Value, json};

const MAX_TITLE: usize = 200;
const MAX_FIELD: usize = 900;
const MAX_FIELDS: usize = 8;

fn shortened(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_owned();
    }
    let kept: String = text.chars().take(most.saturating_sub(1)).collect();
    format!("{kept}…")
}

fn verdict(dispatch: &Dispatch) -> &'static str {
    match dispatch.to {
        AlertStatus::Firing => "Firing",
        AlertStatus::Ok => "Resolved",
        AlertStatus::Pending => "Pending",
        AlertStatus::NoData => "No data",
    }
}

fn title(dispatch: &Dispatch) -> String {
    shortened(
        &format!("{}: {}", verdict(dispatch), dispatch.name),
        MAX_TITLE,
    )
}

fn slack_color(dispatch: &Dispatch) -> &'static str {
    match dispatch.to {
        AlertStatus::Firing if dispatch.severity == AlertSeverity::Critical => "#d92d20",
        AlertStatus::Firing => "#f79009",
        AlertStatus::Ok => "#12b76a",
        AlertStatus::Pending => "#fdb022",
        AlertStatus::NoData => "#98a2b3",
    }
}

fn discord_color(dispatch: &Dispatch) -> u32 {
    u32::from_str_radix(&slack_color(dispatch)[1..], 16).unwrap_or(0x0098_a2b3)
}

fn facts(dispatch: &Dispatch) -> Vec<(String, String)> {
    let mut facts = vec![
        ("Severity".to_owned(), dispatch.severity.as_str().to_owned()),
        (
            "Change".to_owned(),
            format!("{} → {}", dispatch.from.as_str(), dispatch.to.as_str()),
        ),
    ];
    if let Some(value) = dispatch.value.filter(|value| value.is_finite()) {
        facts.push(("Value".to_owned(), format!("{value}")));
    }
    for (key, value) in dispatch.labels.pairs() {
        facts.push((key.to_string(), shortened(value, MAX_FIELD)));
    }
    if let Some(context) = &dispatch.context {
        let pods: Vec<&str> = context.pods().iter().map(|pod| &*pod.name).collect();
        if !pods.is_empty() {
            let more = match context.others() {
                0 => String::new(),
                others => format!(" and {others} more"),
            };
            facts.push(("Pods".to_owned(), format!("{}{more}", pods.join(", "))));
        }
        if !context.versions().is_empty() {
            let versions: Vec<&str> = context.versions().iter().map(AsRef::as_ref).collect();
            facts.push(("Versions".to_owned(), versions.join(", ")));
        }
        if !context.nodes().is_empty() {
            let nodes: Vec<&str> = context.nodes().iter().map(AsRef::as_ref).collect();
            facts.push(("Nodes".to_owned(), nodes.join(", ")));
        }
    }
    facts.truncate(MAX_FIELDS);
    facts
}

fn slack_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn slack(dispatch: &Dispatch, link: &str) -> Value {
    let title = title(dispatch);
    json!({
        "text": slack_text(&title),
        "attachments": [{
            "color": slack_color(dispatch),
            "fallback": slack_text(&title),
            "title": slack_text(&title),
            "title_link": link,
            "fields": facts(dispatch)
                .into_iter()
                .map(|(name, value)| json!({
                    "title": slack_text(&name),
                    "value": slack_text(&value),
                    "short": true,
                }))
                .collect::<Vec<_>>(),
            "ts": dispatch.at_ms / 1_000,
        }],
    })
}

pub fn discord(dispatch: &Dispatch, link: &str) -> Value {
    json!({
        "allowed_mentions": {"parse": []},
        "embeds": [{
            "title": title(dispatch),
            "url": link,
            "color": discord_color(dispatch),
            "fields": facts(dispatch)
                .into_iter()
                .map(|(name, value)| json!({
                    "name": shortened(&name, 200),
                    "value": shortened(&value, MAX_FIELD),
                    "inline": true,
                }))
                .collect::<Vec<_>>(),
        }],
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ferristrace_domain::{AlertSeverity, AlertStatus, Dispatch, Labels, RuleId, TenantId};
    use uuid::Uuid;

    use super::{discord, slack};

    fn dispatch(name: &str, to: AlertStatus) -> Dispatch {
        Dispatch {
            rule: RuleId::new(Uuid::from_u128(7)),
            name: name.into(),
            severity: AlertSeverity::Critical,
            org: TenantId::new(Uuid::from_u128(1)),
            from: AlertStatus::Ok,
            to,
            value: Some(7.5),
            labels: Labels::new(BTreeMap::from([("service".into(), "checkout".into())])),
            at_ms: 1_700_000_000_000,
            context: None,
        }
    }

    #[test]
    fn a_slack_message_is_a_colored_attachment_with_a_link_and_the_facts() {
        let body = slack(
            &dispatch("Checkout errors", AlertStatus::Firing),
            "https://x/a",
        );

        assert_eq!(body["text"], "Firing: Checkout errors");
        assert_eq!(body["attachments"][0]["color"], "#d92d20");
        assert_eq!(body["attachments"][0]["title_link"], "https://x/a");
        let fields = body["attachments"][0]["fields"].as_array().expect("fields");
        assert!(
            fields
                .iter()
                .any(|f| f["title"] == "service" && f["value"] == "checkout")
        );
        assert!(
            fields
                .iter()
                .any(|f| f["title"] == "Value" && f["value"] == "7.5")
        );
    }

    #[test]
    fn a_name_cannot_ping_a_slack_channel() {
        let body = slack(
            &dispatch("<!channel> & <@U1>", AlertStatus::Firing),
            "https://x/a",
        );

        assert_eq!(body["text"], "Firing: &lt;!channel&gt; &amp; &lt;@U1&gt;");
    }

    #[test]
    fn a_discord_message_is_an_embed_that_mentions_nobody() {
        let body = discord(&dispatch("@everyone", AlertStatus::Ok), "https://x/a");

        assert_eq!(body["allowed_mentions"]["parse"], serde_json::json!([]));
        assert_eq!(body["embeds"][0]["title"], "Resolved: @everyone");
        assert_eq!(body["embeds"][0]["color"], 0x0012_b76a);
        assert_eq!(body["embeds"][0]["url"], "https://x/a");
    }

    #[test]
    fn a_very_long_name_is_cut_to_what_the_services_accept() {
        let body = discord(&dispatch(&"a".repeat(1_000), AlertStatus::Firing), "u");

        let title = body["embeds"][0]["title"].as_str().expect("a title");
        assert!(title.chars().count() <= 256);
    }
}
