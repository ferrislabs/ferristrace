use std::fmt;
use std::sync::Arc;

use ferristrace_application::{Ferristrace, Stack};

const FALLBACK_RETENTION_DAYS: u64 = 30;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McpSettings {
    pub gateway_client_id: Option<String>,
    pub public_url: Option<String>,
}

pub struct AppState<S: Stack> {
    pub app: Arc<Ferristrace<S>>,
    pub default_retention_days: u64,
    pub agents_off_reason: Option<String>,
    pub reserved_client_id: Option<String>,
    pub mcp: McpSettings,
}

impl<S: Stack> AppState<S> {
    pub fn new(app: Arc<Ferristrace<S>>) -> Self {
        Self {
            app,
            default_retention_days: FALLBACK_RETENTION_DAYS,
            agents_off_reason: None,
            reserved_client_id: None,
            mcp: McpSettings::default(),
        }
    }

    #[must_use]
    pub fn with_mcp(mut self, mcp: McpSettings) -> Self {
        self.mcp = mcp;
        self
    }

    #[must_use]
    pub fn with_agent_policy(
        mut self,
        off_reason: Option<String>,
        reserved_client_id: Option<String>,
    ) -> Self {
        self.agents_off_reason = off_reason;
        self.reserved_client_id = reserved_client_id;
        self
    }

    #[must_use]
    pub fn with_default_retention_days(mut self, days: u64) -> Self {
        self.default_retention_days = days;
        self
    }
}

impl<S: Stack> Clone for AppState<S> {
    fn clone(&self) -> Self {
        Self {
            app: Arc::clone(&self.app),
            default_retention_days: self.default_retention_days,
            agents_off_reason: self.agents_off_reason.clone(),
            reserved_client_id: self.reserved_client_id.clone(),
            mcp: self.mcp.clone(),
        }
    }
}

impl<S: Stack> fmt::Debug for AppState<S> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppState")
            .field("default_retention_days", &self.default_retention_days)
            .finish_non_exhaustive()
    }
}
