use core::fmt;

use uuid::Uuid;

use super::errors::{BadViewDefinition, BadViewName};

const MAX_NAME_BYTES: usize = 80;
const MAX_DEFINITION_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ViewId(Uuid);

impl ViewId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }

    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl fmt::Display for ViewId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewName(Box<str>);

impl ViewName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for ViewName {
    type Error = BadViewName;

    fn try_from(typed: &str) -> Result<Self, Self::Error> {
        let name = typed.trim();
        if name.is_empty() {
            return Err(BadViewName::Empty);
        }
        if name.len() > MAX_NAME_BYTES {
            return Err(BadViewName::TooLong);
        }
        if name.chars().any(char::is_control) {
            return Err(BadViewName::Control);
        }
        Ok(Self(name.into()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewSignal {
    Traces,
}

impl ViewSignal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Traces => "traces",
        }
    }
}

impl TryFrom<&str> for ViewSignal {
    type Error = ();

    fn try_from(spelling: &str) -> Result<Self, Self::Error> {
        match spelling {
            "traces" => Ok(Self::Traces),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Visibility {
    Private,
    Org,
}

impl Visibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Private => "private",
            Self::Org => "org",
        }
    }
}

impl TryFrom<&str> for Visibility {
    type Error = ();

    fn try_from(spelling: &str) -> Result<Self, Self::Error> {
        match spelling {
            "private" => Ok(Self::Private),
            "org" => Ok(Self::Org),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewDefinition(Box<str>);

impl ViewDefinition {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for ViewDefinition {
    type Error = BadViewDefinition;

    fn try_from(json: &str) -> Result<Self, Self::Error> {
        if json.len() > MAX_DEFINITION_BYTES {
            return Err(BadViewDefinition::TooLarge);
        }
        if !json.trim_start().starts_with('{') {
            return Err(BadViewDefinition::NotAnObject);
        }
        Ok(Self(json.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_trimmed_and_bounded() {
        assert_eq!(
            ViewName::try_from("  Slow checkout ").unwrap().as_str(),
            "Slow checkout"
        );
        assert_eq!(ViewName::try_from("   "), Err(BadViewName::Empty));
        assert_eq!(
            ViewName::try_from(&*"n".repeat(81)),
            Err(BadViewName::TooLong)
        );
        assert_eq!(ViewName::try_from("a\nb"), Err(BadViewName::Control));
    }

    #[test]
    fn a_definition_is_a_json_object_of_bounded_size() {
        assert!(ViewDefinition::try_from(r#"{"q":[]}"#).is_ok());
        assert_eq!(
            ViewDefinition::try_from("[1]"),
            Err(BadViewDefinition::NotAnObject)
        );
        assert_eq!(
            ViewDefinition::try_from(&*format!("{{\"a\":\"{}\"}}", "x".repeat(16 * 1024))),
            Err(BadViewDefinition::TooLarge)
        );
    }

    #[test]
    fn signals_and_visibilities_round_trip_their_spelling() {
        assert_eq!(ViewSignal::try_from("traces"), Ok(ViewSignal::Traces));
        assert!(ViewSignal::try_from("spans").is_err());
        for one in [Visibility::Private, Visibility::Org] {
            assert_eq!(Visibility::try_from(one.as_str()), Ok(one));
        }
        assert!(Visibility::try_from("public").is_err());
    }
}
