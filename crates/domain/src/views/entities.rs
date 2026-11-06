use super::value_objects::{ViewDefinition, ViewId, ViewName, ViewSignal, Visibility};
use crate::identity::AccountId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub id: ViewId,
    pub signal: ViewSignal,
    pub name: Box<str>,
    pub visibility: Visibility,
    pub definition: Box<str>,
    pub created_by: Option<AccountId>,
    pub created_at_unix_nanos: u64,
    pub updated_at_unix_nanos: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewView {
    pub signal: ViewSignal,
    pub name: ViewName,
    pub visibility: Visibility,
    pub definition: ViewDefinition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewChange {
    pub name: ViewName,
    pub definition: ViewDefinition,
    pub visibility: Option<Visibility>,
}
