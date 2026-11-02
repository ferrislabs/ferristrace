use std::fmt;
use std::sync::Arc;

use ferristrace_application::{Ferristrace, Stack};

use crate::session_cookie::SessionCookie;

const FALLBACK_RETENTION_DAYS: u64 = 30;

pub struct AppState<S: Stack> {
    pub app: Arc<Ferristrace<S>>,
    pub cookie: SessionCookie,
    pub default_retention_days: u64,
}

impl<S: Stack> AppState<S> {
    pub fn new(app: Arc<Ferristrace<S>>, cookie: SessionCookie) -> Self {
        Self {
            app,
            cookie,
            default_retention_days: FALLBACK_RETENTION_DAYS,
        }
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
            cookie: self.cookie,
            default_retention_days: self.default_retention_days,
        }
    }
}

impl<S: Stack> fmt::Debug for AppState<S> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppState")
            .field("cookie", &self.cookie)
            .field("default_retention_days", &self.default_retention_days)
            .finish_non_exhaustive()
    }
}
