use core::fmt;

use uuid::Uuid;

use super::errors::BadPostmortem;

pub const MAX_BODY_BYTES: usize = 200 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PostmortemId(Uuid);

impl PostmortemId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }

    pub fn fresh() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl fmt::Display for PostmortemId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PostmortemStatus {
    Draft,
    Published,
}

impl PostmortemStatus {
    pub const ALL: &'static [Self] = &[Self::Draft, Self::Published];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Published => "published",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|held| held.as_str() == text)
    }
}

impl fmt::Display for PostmortemStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostmortemBody(Box<str>);

impl PostmortemBody {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for PostmortemBody {
    type Error = BadPostmortem;

    fn try_from(chosen: &str) -> Result<Self, BadPostmortem> {
        if chosen.trim().is_empty() || chosen.len() > MAX_BODY_BYTES {
            Err(BadPostmortem::Body {
                max: MAX_BODY_BYTES,
            })
        } else {
            Ok(Self(chosen.into()))
        }
    }
}
