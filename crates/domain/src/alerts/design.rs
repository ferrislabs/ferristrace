use super::errors::BadDesign;

pub const MAX_CUSTOM_FIELDS: usize = 5;
pub const MAX_PINGS: usize = 5;
pub const MAX_URL_BYTES: usize = 500;

const PLACEHOLDERS: [&str; 10] = [
    "alert", "severity", "state", "from", "to", "value", "link", "pods", "versions", "nodes",
];
const LABEL_PREFIX: &str = "label.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template(Box<str>);

impl Template {
    pub fn parse(given: &str, most: usize) -> Result<Self, BadDesign> {
        if given.len() > most {
            return Err(BadDesign::TooLong { max: most });
        }
        for name in names_in(given) {
            let known = PLACEHOLDERS.contains(&name)
                || name
                    .strip_prefix(LABEL_PREFIX)
                    .is_some_and(|key| !key.is_empty());
            if !known {
                return Err(BadDesign::Placeholder(name.into()));
            }
        }
        Ok(Self(given.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn render(&self, lookup: impl Fn(&str) -> Option<String>) -> String {
        let text = self.as_str();
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            out.push_str(&rest[..open]);
            let after = &rest[open + 1..];
            match after.find('}') {
                Some(close) if is_name(&after[..close]) => {
                    out.push_str(&lookup(&after[..close]).unwrap_or_default());
                    rest = &after[close + 1..];
                }
                _ => {
                    out.push('{');
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        out
    }
}

fn is_name(candidate: &str) -> bool {
    !candidate.is_empty()
        && candidate
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

fn names_in(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if is_name(&after[..close]) => {
                found.push(&after[..close]);
                rest = &after[close + 1..];
            }
            _ => rest = after,
        }
    }
    found
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomField {
    pub name: Template,
    pub value: Template,
    pub inline: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Colors {
    pub firing: Option<u32>,
    pub resolved: Option<u32>,
    pub pending: Option<u32>,
    pub no_data: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fact {
    Severity,
    Change,
    Value,
    Labels,
    Culprits,
    Link,
    Timestamp,
}

impl Fact {
    pub const ALL: [Self; 7] = [
        Self::Severity,
        Self::Change,
        Self::Value,
        Self::Labels,
        Self::Culprits,
        Self::Link,
        Self::Timestamp,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Severity => "severity",
            Self::Change => "change",
            Self::Value => "value",
            Self::Labels => "labels",
            Self::Culprits => "culprits",
            Self::Link => "link",
            Self::Timestamp => "timestamp",
        }
    }

    fn bit(self) -> u8 {
        1 << (self as u8)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shown(u8);

impl Shown {
    pub fn holds(self, fact: Fact) -> bool {
        self.0 & fact.bit() != 0
    }

    #[must_use]
    pub fn with(self, fact: Fact, shown: bool) -> Self {
        if shown {
            Self(self.0 | fact.bit())
        } else {
            Self(self.0 & !fact.bit())
        }
    }
}

impl Default for Shown {
    fn default() -> Self {
        Fact::ALL.into_iter().fold(Self(0), |shown, fact| {
            shown.with(fact, fact != Fact::Timestamp)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Design {
    pub content: Option<Template>,
    pub title: Option<Template>,
    pub description: Option<Template>,
    pub author: Option<Template>,
    pub footer: Option<Template>,
    pub thumbnail: Option<Box<str>>,
    pub colors: Colors,
    pub shown: Shown,
    pub fields: Vec<CustomField>,
    pub ping_roles: Vec<Box<str>>,
    pub ping_users: Vec<Box<str>>,
}

pub fn snowflake(given: &str) -> Result<Box<str>, BadDesign> {
    let kept = given.trim();
    if (17..=20).contains(&kept.len()) && kept.bytes().all(|b| b.is_ascii_digit()) {
        Ok(kept.into())
    } else {
        Err(BadDesign::Snowflake)
    }
}

pub fn thumbnail(given: &str) -> Result<Box<str>, BadDesign> {
    let kept = given.trim();
    if kept.len() > MAX_URL_BYTES
        || kept.contains(char::is_whitespace)
        || !kept.to_ascii_lowercase().starts_with("https://")
        || kept.len() <= "https://".len()
    {
        return Err(BadDesign::Thumbnail);
    }
    Ok(kept.into())
}

pub fn colour(given: u32) -> Result<u32, BadDesign> {
    if given <= 0x00ff_ffff {
        Ok(given)
    } else {
        Err(BadDesign::Colour)
    }
}

impl Design {
    pub fn checked(self) -> Result<Self, BadDesign> {
        if self.fields.len() > MAX_CUSTOM_FIELDS {
            return Err(BadDesign::TooManyFields {
                max: MAX_CUSTOM_FIELDS,
            });
        }
        if self.ping_roles.len() > MAX_PINGS || self.ping_users.len() > MAX_PINGS {
            return Err(BadDesign::TooManyPings { max: MAX_PINGS });
        }
        for colour_given in [
            self.colors.firing,
            self.colors.resolved,
            self.colors.pending,
            self.colors.no_data,
        ]
        .into_iter()
        .flatten()
        {
            colour(colour_given)?;
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::{BadDesign, Colors, CustomField, Design, Template, colour, snowflake, thumbnail};

    fn template(text: &str) -> Template {
        Template::parse(text, 200).expect("a template")
    }

    #[test]
    fn a_template_fills_known_placeholders_and_keeps_stray_braces() {
        let rendered =
            template("{state}: {alert} {label.service} { 1 } {").render(|name| match name {
                "state" => Some("Firing".into()),
                "alert" => Some("Checkout".into()),
                "label.service" => Some("checkout".into()),
                _ => None,
            });

        assert_eq!(rendered, "Firing: Checkout checkout { 1 } {");
    }

    #[test]
    fn an_unknown_placeholder_is_refused_when_the_template_is_made() {
        assert_eq!(
            Template::parse("{alert} {nope}", 200).err(),
            Some(BadDesign::Placeholder("nope".into()))
        );
        assert_eq!(
            Template::parse("{label.}", 200).err(),
            Some(BadDesign::Placeholder("label.".into()))
        );
        assert!(Template::parse("{label.env} {value} {pods}", 200).is_ok());
    }

    #[test]
    fn a_template_over_its_limit_is_refused() {
        assert_eq!(
            Template::parse(&"a".repeat(201), 200).err(),
            Some(BadDesign::TooLong { max: 200 })
        );
    }

    #[test]
    fn a_thumbnail_is_https_without_spaces() {
        assert!(thumbnail("https://cdn.example.com/a.png").is_ok());
        for bad in [
            "http://cdn.example.com/a.png",
            "https://",
            "https://a b/c.png",
            "javascript:alert(1)",
        ] {
            assert_eq!(thumbnail(bad).err(), Some(BadDesign::Thumbnail), "{bad}");
        }
    }

    #[test]
    fn a_role_or_user_to_ping_is_a_discord_id() {
        assert!(snowflake("123456789012345678").is_ok());
        for bad in [
            "123",
            "everyone",
            "12345678901234567x",
            "123456789012345678901",
        ] {
            assert_eq!(snowflake(bad).err(), Some(BadDesign::Snowflake), "{bad}");
        }
    }

    #[test]
    fn a_colour_fits_in_three_bytes() {
        assert_eq!(colour(0x00ff_ffff), Ok(0x00ff_ffff));
        assert_eq!(colour(0x0100_0000), Err(BadDesign::Colour));
    }

    #[test]
    fn a_design_with_too_many_fields_or_a_bad_colour_is_refused() {
        let field = CustomField {
            name: template("a"),
            value: template("b"),
            inline: true,
        };
        let crowded = Design {
            fields: vec![field; 6],
            ..Design::default()
        };
        let garish = Design {
            colors: Colors {
                firing: Some(0x0200_0000),
                ..Colors::default()
            },
            ..Design::default()
        };

        assert!(matches!(
            crowded.checked(),
            Err(BadDesign::TooManyFields { .. })
        ));
        assert_eq!(garish.checked().err(), Some(BadDesign::Colour));
        assert!(Design::default().checked().is_ok());
    }
}
