use core::fmt;
use std::time::Duration;

use clickhouse::inserter::Inserter;
use clickhouse::{Client, error::Error as ClickhouseError};
use ferristrace_domain::{Clock, Scope, SinkError, Span, SpanSink};
use tokio::sync::Mutex;

use crate::clickhouse::rows::{EventRow, SpanRow};

const SPANS: &str = "spans";
const EVENTS: &str = "span_events";
const DEFAULT_MAX_ROWS: u64 = 100_000;
const DEFAULT_PERIOD: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchLimits {
    pub max_rows: u64,
    pub period: Duration,
}

impl Default for BatchLimits {
    fn default() -> Self {
        Self {
            max_rows: DEFAULT_MAX_ROWS,
            period: DEFAULT_PERIOD,
        }
    }
}

pub struct ClickhouseSink<C> {
    spans: Mutex<Inserter<SpanRow<'static>>>,
    events: Mutex<Inserter<EventRow<'static>>>,
    clock: C,
    retention: Duration,
    limits: BatchLimits,
}

impl<C> fmt::Debug for ClickhouseSink<C> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClickhouseSink")
            .field("retention", &self.retention)
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl<C: Clock> ClickhouseSink<C> {
    pub fn new(client: &Client, limits: BatchLimits, retention: Duration, clock: C) -> Self {
        Self {
            spans: Mutex::new(inserter(client, SPANS, limits)),
            events: Mutex::new(inserter(client, EVENTS, limits)),
            clock,
            retention,
            limits,
        }
    }

    pub async fn flush(&self) -> Result<u64, SinkError> {
        let spans = self.spans.lock().await.force_commit().await;
        let events = self.events.lock().await.force_commit().await;
        let spans = spans.map_err(|error| refused(&error))?;
        let events = events.map_err(|error| refused(&error))?;
        Ok(spans.rows + events.rows)
    }

    pub async fn pending_rows(&self) -> u64 {
        self.spans.lock().await.pending().rows + self.events.lock().await.pending().rows
    }

    pub async fn time_left(&self) -> Option<Duration> {
        let spans = self.spans.lock().await.time_left();
        let events = self.events.lock().await.time_left();
        match (spans, events) {
            (Some(spans), Some(events)) => Some(spans.min(events)),
            (left, None) | (None, left) => left,
        }
    }
}

impl<C: Clock + Send + Sync> SpanSink for ClickhouseSink<C> {
    async fn accept(&self, scope: Scope, spans: Vec<Span>) -> Result<(), SinkError> {
        let expire_at = self
            .clock
            .now()
            .saturating_add_secs(self.retention.as_secs());

        let mut span_rows = Vec::with_capacity(spans.len());
        for span in &spans {
            span_rows.push(SpanRow::of(scope, span, expire_at)?);
        }

        let carried = spans.iter().map(|span| span.events.len()).sum();
        let mut event_rows = Vec::with_capacity(carried);
        for span in &spans {
            for event in &span.events {
                event_rows.push(EventRow::of(scope, span, event, expire_at)?);
            }
        }

        {
            let mut inserter = self.spans.lock().await;
            for row in &span_rows {
                inserter.write(row).await.map_err(|error| refused(&error))?;
            }
            inserter.commit().await.map_err(|error| refused(&error))?;
        }

        if event_rows.is_empty() {
            return Ok(());
        }

        let mut inserter = self.events.lock().await;
        for row in &event_rows {
            inserter.write(row).await.map_err(|error| refused(&error))?;
        }
        inserter.commit().await.map_err(|error| refused(&error))?;
        Ok(())
    }
}

fn inserter<R: clickhouse::Row>(client: &Client, table: &str, limits: BatchLimits) -> Inserter<R> {
    client
        .inserter::<R>(table)
        .with_max_rows(limits.max_rows)
        .with_period(Some(limits.period))
}

fn refused(error: &ClickhouseError) -> SinkError {
    tracing::error!(%error, "the span store refused a batch");
    SinkError::Unavailable
}
