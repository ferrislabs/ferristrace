use super::value_objects::{ApiSecret, KeyId, KeyScopes};
use crate::catalogue::ProjectId;
use crate::identity::AccountId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiKey {
    pub id: KeyId,
    pub project: Option<ProjectId>,
    pub name: Box<str>,
    pub prefix: Box<str>,
    pub scopes: KeyScopes,
    pub created_at_unix_nanos: u64,
    pub created_by: Option<AccountId>,
    pub last_used_at_unix_nanos: Option<u64>,
}

#[derive(Debug)]
pub struct IssuedKey {
    pub key: ApiKey,
    pub secret: ApiSecret,
}
