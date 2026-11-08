use ferristrace_domain::{
    BadDesign, Colors, CustomField, Design, Fact, Shown, Template, colour, snowflake, thumbnail,
};
use serde::{Deserialize, Serialize};

const TEMPLATE_BYTES: usize = 500;
const TEXT_BYTES: usize = 1_800;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct ColorsBody {
    firing: Option<String>,
    resolved: Option<String>,
    pending: Option<String>,
    no_data: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(transparent)]
pub struct ShownBody(std::collections::BTreeMap<String, bool>);

impl ShownBody {
    fn into_shown(self) -> Shown {
        Fact::ALL.into_iter().fold(Shown::default(), |shown, fact| {
            match self.0.get(fact.key()) {
                Some(given) => shown.with(fact, *given),
                None => shown,
            }
        })
    }
}

impl From<Shown> for ShownBody {
    fn from(shown: Shown) -> Self {
        Self(
            Fact::ALL
                .into_iter()
                .map(|fact| (fact.key().to_owned(), shown.holds(fact)))
                .collect(),
        )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FieldBody {
    name: String,
    value: String,
    #[serde(default = "inline_by_default")]
    inline: bool,
}

fn inline_by_default() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct DesignBody {
    content: Option<String>,
    title: Option<String>,
    description: Option<String>,
    author: Option<String>,
    footer: Option<String>,
    thumbnail: Option<String>,
    colors: ColorsBody,
    shown: ShownBody,
    fields: Vec<FieldBody>,
    ping_roles: Vec<String>,
    ping_users: Vec<String>,
}

fn template(given: Option<&str>, most: usize) -> Result<Option<Template>, BadDesign> {
    match given.map(str::trim).filter(|text| !text.is_empty()) {
        None => Ok(None),
        Some(text) => Template::parse(text, most).map(Some),
    }
}

fn hex(given: Option<&str>) -> Result<Option<u32>, BadDesign> {
    let Some(text) = given.map(str::trim).filter(|text| !text.is_empty()) else {
        return Ok(None);
    };
    let digits = text.strip_prefix('#').unwrap_or(text);
    if digits.len() != 6 {
        return Err(BadDesign::Colour);
    }
    let number = u32::from_str_radix(digits, 16).map_err(|_| BadDesign::Colour)?;
    colour(number).map(Some)
}

fn spelled(colour: Option<u32>) -> Option<String> {
    colour.map(|number| format!("#{number:06x}"))
}

impl DesignBody {
    pub fn into_design(self) -> Result<Design, BadDesign> {
        let mut fields = Vec::new();
        for field in &self.fields {
            let (Some(name), Some(value)) = (
                template(Some(&field.name), TEMPLATE_BYTES)?,
                template(Some(&field.value), TEMPLATE_BYTES)?,
            ) else {
                continue;
            };
            fields.push(CustomField {
                name,
                value,
                inline: field.inline,
            });
        }
        let thumbnail = match self.thumbnail.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(given) => Some(thumbnail(given)?),
        };
        Design {
            content: template(self.content.as_deref(), TEXT_BYTES)?,
            title: template(self.title.as_deref(), TEMPLATE_BYTES)?,
            description: template(self.description.as_deref(), TEXT_BYTES)?,
            author: template(self.author.as_deref(), TEMPLATE_BYTES)?,
            footer: template(self.footer.as_deref(), TEMPLATE_BYTES)?,
            thumbnail,
            colors: Colors {
                firing: hex(self.colors.firing.as_deref())?,
                resolved: hex(self.colors.resolved.as_deref())?,
                pending: hex(self.colors.pending.as_deref())?,
                no_data: hex(self.colors.no_data.as_deref())?,
            },
            shown: self.shown.into_shown(),
            fields,
            ping_roles: self
                .ping_roles
                .iter()
                .map(|id| snowflake(id))
                .collect::<Result<_, _>>()?,
            ping_users: self
                .ping_users
                .iter()
                .map(|id| snowflake(id))
                .collect::<Result<_, _>>()?,
        }
        .checked()
    }
}

impl From<&Design> for DesignBody {
    fn from(design: &Design) -> Self {
        let text = |held: &Option<Template>| held.as_ref().map(|one| one.as_str().to_owned());
        Self {
            content: text(&design.content),
            title: text(&design.title),
            description: text(&design.description),
            author: text(&design.author),
            footer: text(&design.footer),
            thumbnail: design.thumbnail.as_deref().map(str::to_owned),
            colors: ColorsBody {
                firing: spelled(design.colors.firing),
                resolved: spelled(design.colors.resolved),
                pending: spelled(design.colors.pending),
                no_data: spelled(design.colors.no_data),
            },
            shown: ShownBody::from(design.shown),
            fields: design
                .fields
                .iter()
                .map(|field| FieldBody {
                    name: field.name.as_str().to_owned(),
                    value: field.value.as_str().to_owned(),
                    inline: field.inline,
                })
                .collect(),
            ping_roles: design.ping_roles.iter().map(ToString::to_string).collect(),
            ping_users: design.ping_users.iter().map(ToString::to_string).collect(),
        }
    }
}
