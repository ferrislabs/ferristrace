use std::fmt;
use std::sync::Arc;

use ferristrace_application::{Ferristrace, Stack};

use crate::session_cookie::SessionCookie;

pub struct AppState<S: Stack> {
    pub app: Arc<Ferristrace<S>>,
    pub cookie: SessionCookie,
}

impl<S: Stack> AppState<S> {
    pub fn new(app: Arc<Ferristrace<S>>, cookie: SessionCookie) -> Self {
        Self { app, cookie }
    }
}

impl<S: Stack> Clone for AppState<S> {
    fn clone(&self) -> Self {
        Self {
            app: Arc::clone(&self.app),
            cookie: self.cookie,
        }
    }
}

impl<S: Stack> fmt::Debug for AppState<S> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppState")
            .field("cookie", &self.cookie)
            .finish_non_exhaustive()
    }
}
