use core::fmt;
use std::time::Duration;

use clickhouse::Client;
use clickhouse::inserter::Inserter;
use ferristrace_domain::{Clock, LogRecord, LogSink, Scope, SinkError};
use tokio::sync::Mutex;

use crate::clickhouse::rows::LogRow;
use crate::clickhouse::sink::{BatchLimits, expiry, inserter, refused};

const LOGS: &str = "logs";

pub struct ClickhouseLogSink<C> {
    logs: Mutex<Inserter<LogRow<'static>>>,
    clock: C,
    retention: Duration,
    limits: BatchLimits,
}

impl<C> fmt::Debug for ClickhouseLogSink<C> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClickhouseLogSink")
            .field("retention", &self.retention)
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl<C: Clock> ClickhouseLogSink<C> {
    pub fn new(client: &Client, limits: BatchLimits, retention: Duration, clock: C) -> Self {
        Self {
            logs: Mutex::new(inserter(client, LOGS, limits)),
            clock,
            retention,
            limits,
        }
    }

    pub async fn flush(&self) -> Result<u64, SinkError> {
        let committed = self
            .logs
            .lock()
            .await
            .force_commit()
            .await
            .map_err(|error| refused(&error))?;
        Ok(committed.rows)
    }

    pub async fn pending_rows(&self) -> u64 {
        self.logs.lock().await.pending().rows
    }

    pub async fn time_left(&self) -> Option<Duration> {
        self.logs.lock().await.time_left()
    }
}

impl<C: Clock + Send + Sync> LogSink for ClickhouseLogSink<C> {
    async fn accept_logs(&self, scope: Scope, records: Vec<LogRecord>) -> Result<(), SinkError> {
        if records.is_empty() {
            return Ok(());
        }
        let expire_at = expiry(self.clock.now(), self.retention, scope.retention);
        let mut rows = Vec::with_capacity(records.len());
        for record in &records {
            rows.push(LogRow::of(scope, record, expire_at)?);
        }

        let mut inserter = self.logs.lock().await;
        for row in &rows {
            inserter.write(row).await.map_err(|error| refused(&error))?;
        }
        inserter.commit().await.map_err(|error| refused(&error))?;
        Ok(())
    }
}
