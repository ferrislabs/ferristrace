use ferristrace_domain::{Colors, CustomField, Design, Fact, Shown, Template};
use serde_json::{Value, json};

const TEMPLATE_BYTES: usize = 4_000;

fn text(template: Option<&Template>) -> Value {
    template.map_or(Value::Null, |template| json!(template.as_str()))
}

fn number(colour: Option<u32>) -> Value {
    colour.map_or(Value::Null, |colour| json!(colour))
}

pub(crate) fn encode(design: &Design) -> Value {
    json!({
        "content": text(design.content.as_ref()),
        "title": text(design.title.as_ref()),
        "description": text(design.description.as_ref()),
        "author": text(design.author.as_ref()),
        "footer": text(design.footer.as_ref()),
        "thumbnail": design.thumbnail,
        "colors": {
            "firing": number(design.colors.firing),
            "resolved": number(design.colors.resolved),
            "pending": number(design.colors.pending),
            "no_data": number(design.colors.no_data),
        },
        "shown": Fact::ALL.iter().map(|fact| (fact.key().to_owned(), json!(design.shown.holds(*fact)))).collect::<serde_json::Map<_, _>>(),
        "fields": design.fields.iter().map(|field| json!({
            "name": field.name.as_str(),
            "value": field.value.as_str(),
            "inline": field.inline,
        })).collect::<Vec<_>>(),
        "ping_roles": design.ping_roles,
        "ping_users": design.ping_users,
        "incident_updates": design.incident_updates,
    })
}

fn template(value: &Value, key: &str) -> Result<Option<Template>, ()> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(held) => held
            .as_str()
            .and_then(|text| Template::parse(text, TEMPLATE_BYTES).ok())
            .map(Some)
            .ok_or(()),
    }
}

fn colour(value: &Value, key: &str) -> Option<u32> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|held| u32::try_from(held).ok())
}

fn flag(value: &Value, key: &str, otherwise: bool) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(otherwise)
}

fn ids(value: &Value, key: &str) -> Option<Vec<Box<str>>> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|held| {
            held.iter()
                .filter_map(|one| one.as_str().map(Box::from))
                .collect()
        })
        .or(Some(Vec::new()))
}

pub(crate) fn decode(held: &Value) -> Option<Design> {
    let shown_held = held.get("shown").unwrap_or(&Value::Null);
    let colors_held = held.get("colors").unwrap_or(&Value::Null);
    let mut fields = Vec::new();
    for one in held
        .get("fields")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        fields.push(CustomField {
            name: template(one, "name").ok()??,
            value: template(one, "value").ok()??,
            inline: flag(one, "inline", true),
        });
    }
    Design {
        content: template(held, "content").ok()?,
        title: template(held, "title").ok()?,
        description: template(held, "description").ok()?,
        author: template(held, "author").ok()?,
        footer: template(held, "footer").ok()?,
        thumbnail: held.get("thumbnail").and_then(Value::as_str).map(Box::from),
        colors: Colors {
            firing: colour(colors_held, "firing"),
            resolved: colour(colors_held, "resolved"),
            pending: colour(colors_held, "pending"),
            no_data: colour(colors_held, "no_data"),
        },
        shown: Fact::ALL.into_iter().fold(Shown::default(), |shown, fact| {
            shown.with(fact, flag(shown_held, fact.key(), shown.holds(fact)))
        }),
        fields,
        ping_roles: ids(held, "ping_roles")?,
        ping_users: ids(held, "ping_users")?,
        incident_updates: flag(held, "incident_updates", false),
    }
    .checked()
    .ok()
}

#[cfg(test)]
mod tests {
    use ferristrace_domain::{Colors, CustomField, Design, Template};

    use super::{decode, encode};

    #[test]
    fn a_design_reads_back_as_it_was_kept() {
        let design = Design {
            title: Some(Template::parse("{state}: {alert}", 100).expect("a template")),
            footer: Some(Template::parse("on {label.env}", 100).expect("a template")),
            thumbnail: Some("https://cdn.example.com/a.png".into()),
            colors: Colors {
                firing: Some(0x00ff_0000),
                ..Colors::default()
            },
            fields: vec![CustomField {
                name: Template::parse("Service", 100).expect("a template"),
                value: Template::parse("{label.service}", 100).expect("a template"),
                inline: false,
            }],
            ping_roles: vec!["123456789012345678".into()],
            ..Design::default()
        };

        assert_eq!(decode(&encode(&design)), Some(design));
    }

    #[test]
    fn an_empty_design_reads_back_as_the_standard_one() {
        assert_eq!(decode(&serde_json::json!({})), Some(Design::default()));
    }
}
