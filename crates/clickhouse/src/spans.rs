use std::num::NonZeroU16;
use std::time::Duration;

use clickhouse::Client;
use ferristrace_domain::Clock;

use crate::clickhouse::{
    BatchLimits, ClickhouseIssues, ClickhouseLogReader, ClickhouseLogSink, ClickhouseMetricReader,
    ClickhouseMetricSink, ClickhouseMigrator, ClickhouseReader, ClickhouseSink, ClickhouseTiering,
    MigrateError, TieringError,
};

const DEFAULT_RETENTION_DAYS: u64 = 30;
const SECONDS_PER_DAY: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanStoreSettings {
    pub url: String,
    pub database: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub retention_days: u64,
    pub cold_after_days: Option<NonZeroU16>,
}

impl SpanStoreSettings {
    pub fn at(url: &str) -> Self {
        Self {
            url: url.to_owned(),
            database: None,
            user: None,
            password: None,
            retention_days: DEFAULT_RETENTION_DAYS,
            cold_after_days: None,
        }
    }

    fn retention(&self) -> Duration {
        Duration::from_secs(self.retention_days.saturating_mul(SECONDS_PER_DAY))
    }

    fn client(&self) -> Client {
        let mut client = Client::default().with_url(&self.url);
        if let Some(database) = &self.database {
            client = client.with_database(database);
        }
        if let Some(user) = &self.user {
            client = client.with_user(user);
        }
        if let Some(password) = &self.password {
            client = client.with_password(password);
        }
        client
    }
}

#[derive(Debug)]
pub struct SpanStore<C> {
    pub sink: ClickhouseSink<C>,
    pub reader: ClickhouseReader,
    pub issues: ClickhouseIssues,
    pub logs: ClickhouseLogSink<C>,
    pub log_reader: ClickhouseLogReader,
    pub metrics: ClickhouseMetricSink<C>,
    pub metric_reader: ClickhouseMetricReader,
}

#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error(transparent)]
    Migrate(#[from] MigrateError),
    #[error(transparent)]
    Tiering(#[from] TieringError),
}

pub async fn open<C: Clock + Clone>(
    settings: &SpanStoreSettings,
    clock: C,
) -> Result<SpanStore<C>, OpenError> {
    let client = settings.client();
    let ran = ClickhouseMigrator::new(client.clone()).up().await?;
    if ran.is_empty() {
        tracing::info!("the span schema was already up to date");
    } else {
        tracing::info!(?ran, "the span schema was migrated");
    }
    ClickhouseTiering::new(client.clone())
        .apply(settings.cold_after_days)
        .await?;
    if let Some(days) = settings.cold_after_days {
        tracing::info!(
            cold_after_days = days.get(),
            "signal parts older than this move to the cold volume"
        );
    }
    Ok(SpanStore {
        metrics: ClickhouseMetricSink::new(
            &client,
            BatchLimits::default(),
            settings.retention(),
            clock.clone(),
        ),
        metric_reader: ClickhouseMetricReader::new(client.clone()),
        logs: ClickhouseLogSink::new(
            &client,
            BatchLimits::default(),
            settings.retention(),
            clock.clone(),
        ),
        log_reader: ClickhouseLogReader::new(client.clone()),
        sink: ClickhouseSink::new(&client, BatchLimits::default(), settings.retention(), clock),
        issues: ClickhouseIssues::new(client.clone()),
        reader: ClickhouseReader::new(client),
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::SpanStoreSettings;

    #[test]
    fn a_span_store_keeps_a_month_by_default() {
        assert_eq!(
            SpanStoreSettings::at("http://localhost:8123").retention(),
            Duration::from_secs(30 * 24 * 60 * 60)
        );
    }

    #[test]
    fn a_retention_long_enough_to_overflow_is_capped_rather_than_wrapped() {
        let settings = SpanStoreSettings {
            retention_days: u64::MAX,
            ..SpanStoreSettings::at("http://localhost:8123")
        };

        assert_eq!(settings.retention(), Duration::from_secs(u64::MAX));
    }
}
