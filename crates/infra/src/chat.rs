use ferristrace_domain::{
    AlertSeverity, AlertStatus, Design, Dispatch, Fact, IncidentNotice, IncidentStatus, Shown,
    Template,
};
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

fn absolute(link: &str) -> Option<&str> {
    (link.starts_with("https://") || link.starts_with("http://")).then_some(link)
}

fn facts(dispatch: &Dispatch, shown: Shown) -> Vec<(String, String)> {
    let mut facts = Vec::new();
    if shown.holds(Fact::Severity) {
        facts.push(("Severity".to_owned(), dispatch.severity.as_str().to_owned()));
    }
    if shown.holds(Fact::Change) {
        facts.push((
            "Change".to_owned(),
            format!("{} → {}", dispatch.from.as_str(), dispatch.to.as_str()),
        ));
    }
    if let Some(value) = dispatch
        .value
        .filter(|value| value.is_finite() && shown.holds(Fact::Value))
    {
        facts.push(("Value".to_owned(), format!("{value}")));
    }
    if shown.holds(Fact::Labels) {
        for (key, value) in dispatch.labels.pairs() {
            facts.push((key.to_string(), shortened(value, MAX_FIELD)));
        }
    }
    if let (Some(context), true) = (&dispatch.context, shown.holds(Fact::Culprits)) {
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
    facts.retain(|(name, value)| !name.trim().is_empty() && !value.trim().is_empty());
    facts.truncate(MAX_FIELDS);
    facts
}

fn slack_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn slack_pings(design: &Design, dispatch: &Dispatch) -> String {
    if dispatch.to != AlertStatus::Firing {
        return String::new();
    }
    let (roles, users) = pinged_ids(design, dispatch);
    let groups = roles
        .iter()
        .filter(|id| id.starts_with('S'))
        .map(|id| format!("<!subteam^{id}>"));
    let people = users
        .iter()
        .filter(|id| id.starts_with(['U', 'W']))
        .map(|id| format!("<@{id}>"));
    groups.chain(people).collect::<Vec<_>>().join(" ")
}

fn slack_fields(design: &Design, dispatch: &Dispatch, link: &str) -> Vec<Value> {
    let mut fields: Vec<Value> = facts(dispatch, design.shown)
        .into_iter()
        .map(|(name, value)| {
            json!({"title": slack_text(&name), "value": slack_text(&value), "short": true})
        })
        .collect();
    for field in &design.fields {
        let name = slack_text(&rendered(&field.name, dispatch, link, 200));
        let value = slack_text(&rendered(&field.value, dispatch, link, MAX_FIELD));
        if !name.is_empty() && !value.is_empty() {
            fields.push(json!({"title": name, "value": value, "short": field.inline}));
        }
    }
    fields.truncate(MAX_EMBED_FIELDS);
    fields
}

pub fn slack(design: &Design, dispatch: &Dispatch, link: &str) -> Value {
    let title = slack_text(&rendered(
        design.title.as_ref().unwrap_or(&default_title()),
        dispatch,
        link,
        MAX_TITLE,
    ));
    let mut attachment = json!({
        "color": slack_colour(design, dispatch),
        "fallback": title,
        "title": title,
        "fields": slack_fields(design, dispatch, link),
        "ts": dispatch.at_ms / 1_000,
    });
    let Some(object) = attachment.as_object_mut() else {
        return Value::Null;
    };
    if let (true, Some(link)) = (design.shown.holds(Fact::Link), absolute(link)) {
        object.insert("title_link".to_owned(), json!(link));
    }
    let mut text = |key: &str, template: &Option<Template>, most: usize| {
        if let Some(template) = template {
            let shown = slack_text(&rendered(template, dispatch, link, most));
            if !shown.is_empty() {
                object.insert(key.to_owned(), json!(shown));
            }
        }
    };
    text("text", &design.description, 3_000);
    text("author_name", &design.author, 256);
    text("footer", &design.footer, 300);
    if let Some(url) = &design.thumbnail {
        object.insert("thumb_url".to_owned(), json!(url));
    }
    let mut lead = slack_pings(design, dispatch);
    if let Some(template) = &design.content {
        let shown = slack_text(&rendered(template, dispatch, link, 1_800));
        if !shown.is_empty() {
            if !lead.is_empty() {
                lead.push(' ');
            }
            lead.push_str(&shown);
        }
    }
    if lead.is_empty() {
        lead = title;
    }
    json!({"text": lead, "attachments": [attachment]})
}

fn slack_colour(design: &Design, dispatch: &Dispatch) -> String {
    format!("#{:06x}", embed_colour(design, dispatch))
}

const DEFAULT_TITLE: &str = "{state}: {alert}";
const MAX_EMBED_FIELDS: usize = 12;

fn neutral(text: &str) -> String {
    text.replace('@', "@\u{200b}").replace('<', "<\u{200b}")
}

fn placeholder(dispatch: &Dispatch, link: &str, name: &str) -> Option<String> {
    let context = dispatch.context.as_ref();
    let joined = |names: Vec<&str>| (!names.is_empty()).then(|| names.join(", "));
    let value = match name {
        "alert" => dispatch.name.to_string(),
        "severity" => dispatch.severity.as_str().to_owned(),
        "state" => verdict(dispatch).to_owned(),
        "from" => dispatch.from.as_str().to_owned(),
        "to" => dispatch.to.as_str().to_owned(),
        "value" => dispatch
            .value
            .filter(|value| value.is_finite())?
            .to_string(),
        "link" => absolute(link)?.to_owned(),
        "pods" => joined(context?.pods().iter().map(|pod| &*pod.name).collect())?,
        "versions" => joined(context?.versions().iter().map(AsRef::as_ref).collect())?,
        "nodes" => joined(context?.nodes().iter().map(AsRef::as_ref).collect())?,
        other => {
            let key = other.strip_prefix("label.")?;
            dispatch
                .labels
                .pairs()
                .iter()
                .find(|(held, _)| &***held == key)
                .map(|(_, value)| value.to_string())?
        }
    };
    Some(neutral(&value))
}

fn rendered(template: &Template, dispatch: &Dispatch, link: &str, most: usize) -> String {
    shortened(
        template
            .render(|name| placeholder(dispatch, link, name))
            .trim(),
        most,
    )
}

fn default_title() -> Template {
    Template::parse(DEFAULT_TITLE, 100).expect("the built-in title is made of known placeholders")
}

fn embed_colour(design: &Design, dispatch: &Dispatch) -> u32 {
    let chosen = match dispatch.to {
        AlertStatus::Firing => design.colors.firing,
        AlertStatus::Ok => design.colors.resolved,
        AlertStatus::Pending => design.colors.pending,
        AlertStatus::NoData => design.colors.no_data,
    };
    chosen.unwrap_or_else(|| discord_color(dispatch))
}

fn iso(at_ms: u64) -> Option<String> {
    let seconds = i64::try_from(at_ms / 1_000).ok()?;
    let at = time::OffsetDateTime::from_unix_timestamp(seconds).ok()?;
    Some(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.000Z",
        at.year(),
        u8::from(at.month()),
        at.day(),
        at.hour(),
        at.minute(),
        at.second()
    ))
}

fn discord_fields(design: &Design, dispatch: &Dispatch, link: &str) -> Vec<Value> {
    let mut fields: Vec<Value> = facts(dispatch, design.shown)
        .into_iter()
        .map(|(name, value)| {
            json!({
                "name": shortened(&name, 200),
                "value": shortened(&value, MAX_FIELD),
                "inline": true,
            })
        })
        .collect();
    for field in &design.fields {
        let name = rendered(&field.name, dispatch, link, 200);
        let value = rendered(&field.value, dispatch, link, MAX_FIELD);
        if !name.is_empty() && !value.is_empty() {
            fields.push(json!({"name": name, "value": value, "inline": field.inline}));
        }
    }
    fields.truncate(MAX_EMBED_FIELDS);
    fields
}

fn merged(first: &[Box<str>], second: &[Box<str>]) -> Vec<Box<str>> {
    let mut ids: Vec<Box<str>> = first.to_vec();
    for id in second {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    ids
}

fn pinged_ids(design: &Design, dispatch: &Dispatch) -> (Vec<Box<str>>, Vec<Box<str>>) {
    (
        merged(&design.ping_roles, dispatch.mentions.roles()),
        merged(&design.ping_users, dispatch.mentions.users()),
    )
}

fn numeric(ids: Vec<Box<str>>) -> Vec<Box<str>> {
    ids.into_iter()
        .filter(|id| id.bytes().all(|byte| byte.is_ascii_digit()))
        .collect()
}

fn pinged(design: &Design, dispatch: &Dispatch) -> (String, Value) {
    let (roles, users) = pinged_ids(design, dispatch);
    let (roles, users) = (numeric(roles), numeric(users));
    if dispatch.to != AlertStatus::Firing || (roles.is_empty() && users.is_empty()) {
        return (String::new(), json!({"parse": []}));
    }
    let mentions: Vec<String> = roles
        .iter()
        .map(|id| format!("<@&{id}>"))
        .chain(users.iter().map(|id| format!("<@{id}>")))
        .collect();
    (
        mentions.join(" "),
        json!({"parse": [], "roles": roles, "users": users}),
    )
}

pub fn discord(design: &Design, dispatch: &Dispatch, link: &str) -> Value {
    let title = design.title.clone().unwrap_or_else(default_title);
    let mut embed = json!({
        "title": rendered(&title, dispatch, link, 256),
        "color": embed_colour(design, dispatch),
        "fields": discord_fields(design, dispatch, link),
    });
    let Some(object) = embed.as_object_mut() else {
        return Value::Null;
    };
    if let Some(template) = &design.description {
        let text = rendered(template, dispatch, link, 4_000);
        if !text.is_empty() {
            object.insert("description".to_owned(), json!(text));
        }
    }
    if let (true, Some(link)) = (design.shown.holds(Fact::Link), absolute(link)) {
        object.insert("url".to_owned(), json!(link));
    }
    if let Some(template) = &design.author {
        let text = rendered(template, dispatch, link, 256);
        if !text.is_empty() {
            object.insert("author".to_owned(), json!({"name": text}));
        }
    }
    if let Some(template) = &design.footer {
        let text = rendered(template, dispatch, link, 2_000);
        if !text.is_empty() {
            object.insert("footer".to_owned(), json!({"text": text}));
        }
    }
    if let Some(url) = &design.thumbnail {
        object.insert("thumbnail".to_owned(), json!({"url": url}));
    }
    if let (true, Some(stamp)) = (design.shown.holds(Fact::Timestamp), iso(dispatch.at_ms)) {
        object.insert("timestamp".to_owned(), json!(stamp));
    }
    let (pings, allowed) = pinged(design, dispatch);
    let mut content = pings;
    if let Some(template) = &design.content {
        let text = rendered(template, dispatch, link, 1_800);
        if !text.is_empty() {
            if !content.is_empty() {
                content.push(' ');
            }
            content.push_str(&text);
        }
    }
    let mut body = json!({"allowed_mentions": allowed, "embeds": [embed]});
    if let (false, Some(fields)) = (content.is_empty(), body.as_object_mut()) {
        fields.insert("content".to_owned(), json!(content));
    }
    body
}

fn incident_title(notice: &IncidentNotice) -> String {
    shortened(
        &format!("[{}] {}", notice.severity.as_str(), notice.title),
        MAX_TITLE,
    )
}

fn incident_color(status: IncidentStatus) -> &'static str {
    match status {
        IncidentStatus::Investigating => "#d92d20",
        IncidentStatus::Identified => "#f79009",
        IncidentStatus::Monitoring => "#2e90fa",
        IncidentStatus::Resolved => "#12b76a",
    }
}

fn incident_text(notice: &IncidentNotice) -> String {
    match &notice.text {
        Some(text) => shortened(&format!("{}\n{}", notice.headline, text), 1_500),
        None => notice.headline.to_string(),
    }
}

pub fn slack_incident(notice: &IncidentNotice, link: &str) -> Value {
    let title = incident_title(notice);
    let mut body = json!({
        "text": slack_text(&format!("{} · {}", title, notice.headline)),
        "attachments": [{
            "color": incident_color(notice.status),
            "title": slack_text(&title),
            "text": slack_text(&incident_text(notice)),
            "fields": [
                {"title": "Status", "value": notice.status.as_str(), "short": true},
                {"title": "Severity", "value": notice.severity.as_str(), "short": true},
            ],
            "ts": notice.at_ms / 1_000,
        }],
    });
    if let (Some(link), Some(attachment)) = (absolute(link), body["attachments"][0].as_object_mut())
    {
        attachment.insert("title_link".to_owned(), json!(link));
    }
    body
}

pub fn discord_incident(notice: &IncidentNotice, link: &str) -> Value {
    let color = u32::from_str_radix(&incident_color(notice.status)[1..], 16).unwrap_or(0x0098_a2b3);
    let mut embed = json!({
        "title": incident_title(notice),
        "description": neutral(&incident_text(notice)),
        "color": color,
        "fields": [
            {"name": "Status", "value": notice.status.as_str(), "inline": true},
            {"name": "Severity", "value": notice.severity.as_str(), "inline": true},
        ],
    });
    if let (Some(link), Some(object)) = (absolute(link), embed.as_object_mut()) {
        object.insert("url".to_owned(), json!(link));
    }
    if let (Some(stamp), Some(object)) = (iso(notice.at_ms), embed.as_object_mut()) {
        object.insert("timestamp".to_owned(), json!(stamp));
    }
    json!({"allowed_mentions": {"parse": []}, "embeds": [embed]})
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ferristrace_domain::{
        AlertSeverity, AlertStatus, Colors, CustomField, Design, Dispatch, Fact, Labels, RuleId,
        Shown, Template, TenantId,
    };
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
            mentions: ferristrace_domain::Mentions::default(),
        }
    }

    #[test]
    fn a_slack_message_is_a_colored_attachment_with_a_link_and_the_facts() {
        let body = slack(
            &Design::default(),
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

    fn slack_design() -> Design {
        Design {
            content: Some(Template::parse("on call: {label.service}", 200).expect("template")),
            title: Some(Template::parse("{alert} is {state}", 200).expect("template")),
            description: Some(Template::parse("{value} errors", 200).expect("template")),
            author: Some(Template::parse("FerrisTrace", 200).expect("template")),
            footer: Some(Template::parse("prod", 200).expect("template")),
            thumbnail: Some("https://cdn.example.com/a.png".into()),
            colors: ferristrace_domain::Colors {
                firing: Some(0x0012_3456),
                ..ferristrace_domain::Colors::default()
            },
            fields: vec![ferristrace_domain::CustomField {
                name: Template::parse("Team", 200).expect("template"),
                value: Template::parse("{label.service}", 200).expect("template"),
                inline: false,
            }],
            ping_roles: vec!["S0614TZR7".into(), "123456789012345678".into()],
            ping_users: vec!["U024BE7LH".into(), "223456789012345678".into()],
            ..Design::default()
        }
    }

    #[test]
    fn a_slack_design_shapes_the_attachment_and_pings_only_slack_ids() {
        let body = slack(
            &slack_design(),
            &dispatch("Checkout errors", AlertStatus::Firing),
            "https://x/a",
        );

        let attachment = &body["attachments"][0];
        assert_eq!(
            body["text"],
            "<!subteam^S0614TZR7> <@U024BE7LH> on call: checkout"
        );
        assert_eq!(attachment["title"], "Checkout errors is Firing");
        assert_eq!(attachment["color"], "#123456");
        assert_eq!(attachment["text"], "7.5 errors");
        assert_eq!(attachment["author_name"], "FerrisTrace");
        assert_eq!(attachment["footer"], "prod");
        assert_eq!(attachment["thumb_url"], "https://cdn.example.com/a.png");
        let fields = attachment["fields"].as_array().expect("fields");
        assert!(
            fields
                .iter()
                .any(|f| f["title"] == "Team" && f["value"] == "checkout" && f["short"] == false)
        );
    }

    #[test]
    fn a_slack_design_pings_only_when_the_alert_fires() {
        let body = slack(
            &slack_design(),
            &dispatch("Checkout errors", AlertStatus::Ok),
            "https://x/a",
        );

        assert_eq!(body["text"], "on call: checkout");
    }

    #[test]
    fn a_discord_message_never_carries_a_slack_id() {
        let body = discord(
            &slack_design(),
            &dispatch("Checkout errors", AlertStatus::Firing),
            "https://x/a",
        );

        assert_eq!(
            body["content"],
            "<@&123456789012345678> <@223456789012345678> on call: checkout"
        );
        assert_eq!(
            body["allowed_mentions"]["roles"],
            serde_json::json!(["123456789012345678"])
        );
    }

    #[test]
    fn a_placeholder_cannot_mention_anyone_in_slack() {
        let design = Design {
            content: Some(Template::parse("{alert}", 200).expect("template")),
            ..Design::default()
        };

        let body = slack(
            &design,
            &dispatch("<!channel>", AlertStatus::Firing),
            "https://x/a",
        );

        assert!(!body["text"].as_str().expect("text").contains("<!channel>"));
    }

    #[test]
    fn a_name_cannot_ping_a_slack_channel() {
        let body = slack(
            &Design::default(),
            &dispatch("<!channel> & <@U1>", AlertStatus::Firing),
            "https://x/a",
        );

        assert_eq!(
            body["text"],
            "Firing: &lt;\u{200b}!channel&gt; &amp; &lt;\u{200b}@\u{200b}U1&gt;"
        );
    }

    #[test]
    fn a_discord_message_is_an_embed_that_mentions_nobody() {
        let body = discord(
            &Design::default(),
            &dispatch("@everyone", AlertStatus::Ok),
            "https://x/a",
        );

        assert_eq!(body["allowed_mentions"]["parse"], serde_json::json!([]));
        assert_eq!(body["embeds"][0]["title"], "Resolved: @\u{200b}everyone");
        assert_eq!(body["embeds"][0]["color"], 0x0012_b76a);
        assert_eq!(body["embeds"][0]["url"], "https://x/a");
    }

    #[test]
    fn a_link_that_is_not_absolute_is_left_out_because_discord_refuses_the_message() {
        let firing = dispatch("Checkout errors", AlertStatus::Firing);

        let embed = discord(&Design::default(), &firing, "/alerts?rule=7");
        let attachment = slack(&Design::default(), &firing, "/alerts?rule=7");

        assert!(embed["embeds"][0].get("url").is_none());
        assert!(attachment["attachments"][0].get("title_link").is_none());
    }

    #[test]
    fn a_label_without_a_value_is_not_sent_as_an_empty_field() {
        let mut firing = dispatch("Checkout errors", AlertStatus::Firing);
        firing.labels = Labels::new(BTreeMap::from([("env".into(), String::new().into())]));

        let body = discord(&Design::default(), &firing, "https://x/a");

        let fields = body["embeds"][0]["fields"].as_array().expect("fields");
        assert!(fields.iter().all(|field| field["name"] != "env"));
    }

    #[test]
    fn a_very_long_name_is_cut_to_what_the_services_accept() {
        let body = discord(
            &Design::default(),
            &dispatch(&"a".repeat(1_000), AlertStatus::Firing),
            "u",
        );

        let title = body["embeds"][0]["title"].as_str().expect("a title");
        assert!(title.chars().count() <= 256);
    }

    fn template(text: &str) -> Template {
        Template::parse(text, 500).expect("a template")
    }

    fn designed() -> Design {
        Design {
            content: Some(template("on call for {label.service}")),
            title: Some(template("{state} · {alert}")),
            description: Some(template("{value} errors per minute on {pods}")),
            author: Some(template("FerrisTrace")),
            footer: Some(template("severity {severity}")),
            thumbnail: Some("https://cdn.example.com/logo.png".into()),
            colors: Colors {
                firing: Some(0x00ff_0000),
                ..Colors::default()
            },
            fields: vec![CustomField {
                name: template("Service"),
                value: template("{label.service}"),
                inline: false,
            }],
            ping_roles: vec!["123456789012345678".into()],
            ping_users: vec!["223456789012345678".into()],
            ..Design::default()
        }
    }

    #[test]
    fn spec_l13_9_a_design_renders_into_the_delivered_embed() {
        let mut firing = dispatch("Checkout errors", AlertStatus::Firing);
        firing.context = None;
        let design = Design {
            shown: Shown::default().with(Fact::Timestamp, true),
            ..designed()
        };

        let body = discord(&design, &firing, "https://x/a");

        let embed = &body["embeds"][0];
        assert_eq!(embed["title"], "Firing · Checkout errors");
        assert_eq!(embed["description"], "7.5 errors per minute on");
        assert_eq!(embed["color"], 0x00ff_0000);
        assert_eq!(embed["author"]["name"], "FerrisTrace");
        assert_eq!(embed["footer"]["text"], "severity critical");
        assert_eq!(
            embed["thumbnail"]["url"],
            "https://cdn.example.com/logo.png"
        );
        assert_eq!(embed["timestamp"], "2023-11-14T22:13:20.000Z");
        let fields = embed["fields"].as_array().expect("fields");
        assert!(
            fields.iter().any(|f| f["name"] == "Service"
                && f["value"] == "checkout"
                && f["inline"] == false)
        );
        assert_eq!(
            body["content"],
            "<@&123456789012345678> <@223456789012345678> on call for checkout"
        );
        assert_eq!(
            body["allowed_mentions"],
            serde_json::json!({
                "parse": [],
                "roles": ["123456789012345678"],
                "users": ["223456789012345678"],
            })
        );
    }

    #[test]
    fn the_pings_are_sent_only_when_the_alert_fires() {
        let resolved = discord(&designed(), &dispatch("Checkout", AlertStatus::Ok), "u");

        assert_eq!(resolved["content"], "on call for checkout");
        assert_eq!(
            resolved["allowed_mentions"],
            serde_json::json!({"parse": []})
        );
    }

    #[test]
    fn spec_l13_10_alert_data_cannot_mention_anyone_through_a_placeholder() {
        let mut hostile = dispatch("<@&1> @everyone @here", AlertStatus::Firing);
        hostile.labels = Labels::new(BTreeMap::from([("service".into(), "@everyone".into())]));
        let design = Design {
            content: Some(template("{alert} {label.service}")),
            description: Some(template("{alert}")),
            ..Design::default()
        };

        let body = discord(&design, &hostile, "u");

        let content = body["content"].as_str().expect("content");
        let described = body["embeds"][0]["description"].as_str().expect("text");
        for text in [content, described] {
            assert!(
                !text.contains("@everyone") && !text.contains("@here"),
                "{text}"
            );
            assert!(!text.contains("<@"), "{text}");
        }
        assert_eq!(body["allowed_mentions"], serde_json::json!({"parse": []}));
    }

    #[test]
    fn without_a_design_the_message_is_the_standard_one() {
        let body = discord(
            &Design::default(),
            &dispatch("Checkout errors", AlertStatus::Firing),
            "https://x/a",
        );

        assert!(body.get("content").is_none());
        assert_eq!(body["embeds"][0]["title"], "Firing: Checkout errors");
        assert!(body["embeds"][0].get("footer").is_none());
        assert!(body["embeds"][0].get("timestamp").is_none());
    }

    #[test]
    fn spec_l13_12_the_rule_mentions_join_the_channel_pings_once_and_only_on_firing() {
        let mut firing = dispatch("Checkout", AlertStatus::Firing);
        firing.mentions = ferristrace_domain::Mentions::new(
            &[
                "123456789012345678".to_owned(),
                "323456789012345678".to_owned(),
            ],
            &["423456789012345678".to_owned()],
        )
        .expect("ids");
        let mut resolved = firing.clone();
        resolved.to = AlertStatus::Ok;

        let loud = discord(&designed(), &firing, "u");
        let quiet = discord(&designed(), &resolved, "u");
        let bare = discord(&Design::default(), &firing, "u");

        assert_eq!(
            loud["allowed_mentions"]["roles"],
            serde_json::json!(["123456789012345678", "323456789012345678"])
        );
        assert_eq!(
            loud["allowed_mentions"]["users"],
            serde_json::json!(["223456789012345678", "423456789012345678"])
        );
        assert!(loud["content"].as_str().expect("content").starts_with("<@&123456789012345678> <@&323456789012345678> <@223456789012345678> <@423456789012345678>"));
        assert_eq!(quiet["allowed_mentions"], serde_json::json!({"parse": []}));
        assert_eq!(
            bare["content"],
            "<@&123456789012345678> <@&323456789012345678> <@423456789012345678>"
        );
    }

    fn notice(headline: &str, text: Option<&str>) -> ferristrace_domain::IncidentNotice {
        ferristrace_domain::IncidentNotice {
            org: TenantId::new(Uuid::from_u128(1)),
            incident: ferristrace_domain::IncidentId::new(Uuid::from_u128(5)),
            title: "Checkout errors".into(),
            severity: ferristrace_domain::IncidentSeverity::Sev2,
            status: ferristrace_domain::IncidentStatus::Identified,
            kind: ferristrace_domain::EntryKind::Comment,
            headline: headline.into(),
            text: text.map(Into::into),
            at_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn an_incident_update_is_an_embed_with_its_status_severity_link_and_text() {
        let body = super::discord_incident(
            &notice("New comment", Some("rolled back")),
            "https://x/incidents/5",
        );

        let embed = &body["embeds"][0];
        assert_eq!(embed["title"], "[sev2] Checkout errors");
        assert_eq!(embed["description"], "New comment\nrolled back");
        assert_eq!(embed["color"], 0x00f7_9009);
        assert_eq!(embed["url"], "https://x/incidents/5");
        assert_eq!(body["allowed_mentions"], serde_json::json!({"parse": []}));
    }

    #[test]
    fn a_comment_cannot_mention_anyone_through_an_incident_update() {
        let hostile = notice("New comment", Some("@everyone <@&123> <!channel>"));

        let discord = super::discord_incident(&hostile, "u");
        let slack = super::slack_incident(&hostile, "u");

        let described = discord["embeds"][0]["description"].as_str().expect("text");
        assert!(!described.contains("@everyone") && !described.contains("<@"));
        let text = slack["attachments"][0]["text"].as_str().expect("text");
        assert!(!text.contains('<') && text.contains("&lt;!channel&gt;"));
    }

    #[test]
    fn a_slack_incident_update_leaves_out_a_link_that_is_not_absolute() {
        let body = super::slack_incident(&notice("Status", None), "/incidents/5");

        assert!(body["attachments"][0].get("title_link").is_none());
    }
}
