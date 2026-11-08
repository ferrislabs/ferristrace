use super::value_objects::{InboxItemId, InboxItemKind};
use crate::identity::AccountId;
use crate::telemetry::TenantId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxItem {
    pub id: InboxItemId,
    pub account: AccountId,
    pub org: TenantId,
    pub kind: InboxItemKind,
    pub title: Box<str>,
    pub body: Box<str>,
    pub link: Option<Box<str>>,
    pub created_ms: u64,
    pub read_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxItemDraft {
    pub account: AccountId,
    pub org: TenantId,
    pub kind: InboxItemKind,
    pub title: Box<str>,
    pub body: Box<str>,
    pub link: Option<Box<str>>,
}
