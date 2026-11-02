use super::value_objects::{Action, Actor, Cursor, Detail, EntryId, PageSize, Target};
use crate::telemetry::TenantId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    pub org: TenantId,
    pub actor: Actor,
    pub action: Action,
    pub target: Target,
    pub detail: Detail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Logged {
    pub id: EntryId,
    pub at_unix_nanos: u64,
    pub entry: AuditEntry,
    pub label: Option<Box<str>>,
}

impl Logged {
    pub fn cursor(&self) -> Cursor {
        Cursor {
            at_unix_nanos: self.at_unix_nanos,
            id: self.id,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditPage {
    pub entries: Vec<Logged>,
    pub next: Option<Cursor>,
}

impl AuditPage {
    pub fn cut(mut probed: Vec<Logged>, page: PageSize) -> Self {
        let kept = usize::try_from(page.get()).unwrap_or(usize::MAX);
        if probed.len() <= kept {
            return Self {
                entries: probed,
                next: None,
            };
        }
        probed.truncate(kept);
        let next = probed.last().map(Logged::cursor);
        Self {
            entries: probed,
            next,
        }
    }
}
