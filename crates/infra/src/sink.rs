use core::future::Future;
use std::collections::VecDeque;
use std::sync::Mutex;

use ferristrace_domain::{LogRecord, LogSink, Scope, SinkError, Span, SpanSink};

const DEFAULT_RETENTION: usize = 1024;

#[derive(Debug, Default)]
struct State {
    accepted: u64,
    recent: VecDeque<(Scope, Span)>,
    logs_accepted: u64,
    recent_logs: VecDeque<(Scope, LogRecord)>,
}

#[derive(Debug)]
pub struct MemorySink {
    retention: usize,
    state: Mutex<State>,
}

impl Default for MemorySink {
    fn default() -> Self {
        Self::with_retention(DEFAULT_RETENTION)
    }
}

impl MemorySink {
    pub fn with_retention(retention: usize) -> Self {
        Self {
            retention,
            state: Mutex::new(State::default()),
        }
    }

    pub fn accepted(&self) -> u64 {
        self.state.lock().expect("sink state").accepted
    }

    pub fn recent(&self) -> Vec<(Scope, Span)> {
        self.state
            .lock()
            .expect("sink state")
            .recent
            .iter()
            .cloned()
            .collect()
    }

    pub fn logs_accepted(&self) -> u64 {
        self.state.lock().expect("sink state").logs_accepted
    }

    pub fn recent_logs(&self) -> Vec<(Scope, LogRecord)> {
        self.state
            .lock()
            .expect("sink state")
            .recent_logs
            .iter()
            .cloned()
            .collect()
    }

    fn push_logs(&self, scope: Scope, records: Vec<LogRecord>) {
        let mut state = self.state.lock().expect("sink state");
        state.logs_accepted += records.len() as u64;
        if self.retention == 0 {
            return;
        }
        for record in records {
            while state.recent_logs.len() >= self.retention {
                state.recent_logs.pop_front();
            }
            state.recent_logs.push_back((scope, record));
        }
    }

    fn push(&self, scope: Scope, spans: Vec<Span>) {
        let mut state = self.state.lock().expect("sink state");
        state.accepted += spans.len() as u64;
        if self.retention == 0 {
            return;
        }
        for span in spans {
            while state.recent.len() >= self.retention {
                state.recent.pop_front();
            }
            state.recent.push_back((scope, span));
        }
    }
}

impl SpanSink for MemorySink {
    fn accept(
        &self,
        scope: Scope,
        spans: Vec<Span>,
    ) -> impl Future<Output = Result<(), SinkError>> + Send {
        self.push(scope, spans);
        async { Ok(()) }
    }
}

impl LogSink for MemorySink {
    fn accept_logs(
        &self,
        scope: Scope,
        records: Vec<LogRecord>,
    ) -> impl Future<Output = Result<(), SinkError>> + Send {
        self.push_logs(scope, records);
        async { Ok(()) }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ferristrace_domain::{
        LogRecord, LogSink, Scope, Severity, Span, SpanId, SpanKind, SpanSink, SpanStatus,
        TenantId, TraceId,
    };
    use uuid::Uuid;

    use super::MemorySink;

    fn a_span(seed: u8) -> Span {
        Span {
            trace_id: TraceId::try_from([seed; 16].as_slice()).expect("sixteen bytes"),
            span_id: SpanId::try_from([seed; 8].as_slice()).expect("eight bytes"),
            parent: None,
            name: "work".into(),
            service_name: "orders".into(),
            kind: SpanKind::Internal,
            status: SpanStatus::Unset,
            start_unix_nanos: 0,
            end_unix_nanos: 1,
            attributes: BTreeMap::new(),
            events: Vec::new(),
        }
    }

    #[tokio::test]
    async fn accepted_spans_are_counted_and_retained() {
        let tenant = TenantId::new(Uuid::from_u128(5));
        let sink = MemorySink::default();

        sink.accept(Scope::of(tenant), vec![a_span(1), a_span(2)])
            .await
            .expect("the sink accepts");

        assert_eq!(sink.accepted(), 2);
        assert_eq!(sink.recent().len(), 2);
        assert_eq!(sink.recent()[0].0, Scope::of(tenant));
    }

    #[tokio::test]
    async fn retention_bounds_what_is_kept() {
        let tenant = TenantId::new(Uuid::from_u128(5));
        let sink = MemorySink::with_retention(1);

        sink.accept(Scope::of(tenant), vec![a_span(1), a_span(2)])
            .await
            .expect("the sink accepts");

        assert_eq!(sink.accepted(), 2);
        assert_eq!(sink.recent().len(), 1);
        assert_eq!(sink.recent()[0].1, a_span(2));
    }

    #[tokio::test]
    async fn zero_retention_still_counts() {
        let tenant = TenantId::new(Uuid::from_u128(5));
        let sink = MemorySink::with_retention(0);

        sink.accept(Scope::of(tenant), vec![a_span(1)])
            .await
            .expect("the sink accepts");

        assert_eq!(sink.accepted(), 1);
        assert!(sink.recent().is_empty());
    }

    fn a_log(body: &str) -> LogRecord {
        LogRecord {
            time_unix_nanos: 1,
            observed_unix_nanos: 1,
            severity: Severity::Info,
            severity_number: 9,
            service_name: "orders".into(),
            body: body.into(),
            attributes: BTreeMap::new(),
            resource: BTreeMap::new(),
            trace_id: None,
            span_id: None,
        }
    }

    #[tokio::test]
    async fn accepted_logs_are_counted_and_retained_apart_from_spans() {
        let tenant = TenantId::new(Uuid::from_u128(5));
        let sink = MemorySink::with_retention(1);

        sink.accept_logs(Scope::of(tenant), vec![a_log("first"), a_log("second")])
            .await
            .expect("the sink accepts");

        assert_eq!(sink.logs_accepted(), 2);
        assert_eq!(sink.accepted(), 0);
        assert_eq!(sink.recent_logs().len(), 1);
        assert_eq!(&*sink.recent_logs()[0].1.body, "second");
    }
}
