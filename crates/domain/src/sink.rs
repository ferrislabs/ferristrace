use core::future::Future;

use crate::span::Span;
use crate::tenant::Scope;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SinkError {
    #[error("the span sink is unavailable")]
    Unavailable,
    #[error("a span timestamp falls outside the range the span store can hold")]
    TimestampOutOfRange,
    #[error("the sink accepts at most {limit} spans per batch")]
    BatchTooLarge { limit: usize },
}

pub trait SpanSink {
    fn accept(
        &self,
        scope: Scope,
        spans: Vec<Span>,
    ) -> impl Future<Output = Result<(), SinkError>> + Send;
}

impl<T: SpanSink + Sync> SpanSink for &T {
    fn accept(
        &self,
        scope: Scope,
        spans: Vec<Span>,
    ) -> impl Future<Output = Result<(), SinkError>> + Send {
        (*self).accept(scope, spans)
    }
}

impl<T: SpanSink + Send + Sync> SpanSink for std::sync::Arc<T> {
    fn accept(
        &self,
        scope: Scope,
        spans: Vec<Span>,
    ) -> impl Future<Output = Result<(), SinkError>> + Send {
        self.as_ref().accept(scope, spans)
    }
}
