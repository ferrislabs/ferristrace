use core::fmt;
use uuid::Uuid;

use super::errors::BadProjectName;
use crate::organisation::TeamId;

pub(super) const MAX_NAME_BYTES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectName(Box<str>);

impl ProjectName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for ProjectName {
    type Error = BadProjectName;

    fn try_from(chosen: &str) -> Result<Self, Self::Error> {
        let kept = chosen.trim().to_ascii_lowercase();
        if kept.len() > MAX_NAME_BYTES {
            return Err(BadProjectName::TooLong(MAX_NAME_BYTES));
        }
        if kept.is_empty() {
            return Err(BadProjectName::Blank);
        }
        let shaped = kept
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            && !kept.starts_with('-')
            && !kept.ends_with('-');
        if shaped {
            Ok(Self(kept.into()))
        } else {
            Err(BadProjectName::Shape)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProjectId(Uuid);

impl ProjectId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }

    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl fmt::Display for ProjectId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerTeam {
    pub id: TeamId,
    pub name: Box<str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Created {
    Done,
    Taken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Renamed {
    Done,
    Taken,
    Gone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assigned {
    Done,
    Gone,
    NoSuchTeam,
}
