use core::fmt;
use std::time::Duration;

use clickhouse::Client;
use clickhouse::inserter::Inserter;
use ferristrace_domain::{
    Clock, Metric, MetricData, MetricPoint, MetricSink, Scope, SinkError, Temporality, UnixSeconds,
};
use tokio::sync::Mutex;

use crate::clickhouse::metric_rows::{
    CatalogueRow, ExemplarRow, HistogramRow, PointKindRow, PointRow, Shape,
};
use crate::clickhouse::sink::{BatchLimits, expiry, inserter, refused};

const POINTS: &str = "metric_points";
const HISTOGRAMS: &str = "metric_histograms";
const EXEMPLARS: &str = "metric_exemplars";
const CATALOGUE: &str = "metric_catalogue";

pub struct ClickhouseMetricSink<C> {
    points: Mutex<Inserter<PointRow<'static>>>,
    histograms: Mutex<Inserter<HistogramRow<'static>>>,
    exemplars: Mutex<Inserter<ExemplarRow<'static>>>,
    catalogue: Mutex<Inserter<CatalogueRow<'static>>>,
    clock: C,
    retention: Duration,
    limits: BatchLimits,
}

impl<C> fmt::Debug for ClickhouseMetricSink<C> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClickhouseMetricSink")
            .field("retention", &self.retention)
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

#[derive(Default)]
struct Rows<'a> {
    points: Vec<PointRow<'a>>,
    histograms: Vec<HistogramRow<'a>>,
    exemplars: Vec<ExemplarRow<'a>>,
    catalogue: Vec<CatalogueRow<'a>>,
}

impl<'a> Rows<'a> {
    fn of(metrics: &'a [Metric], scope: Scope, expire_at: UnixSeconds) -> Result<Self, SinkError> {
        let mut rows = Self::default();
        for metric in metrics {
            rows.catalogue
                .push(CatalogueRow::of(scope, metric, expire_at)?);
            match &metric.data {
                MetricData::Gauge(points) => {
                    let shape = Shape {
                        kind: PointKindRow::Gauge,
                        temporality: Temporality::Unspecified,
                        monotonic: false,
                    };
                    rows.numbers(scope, metric, shape, points, expire_at)?;
                }
                MetricData::Sum {
                    monotonic,
                    temporality,
                    points,
                } => {
                    let shape = Shape {
                        kind: PointKindRow::Sum,
                        temporality: *temporality,
                        monotonic: *monotonic,
                    };
                    rows.numbers(scope, metric, shape, points, expire_at)?;
                }
                MetricData::Histogram {
                    temporality,
                    points,
                } => {
                    for point in points {
                        rows.histograms.push(HistogramRow::of(
                            scope,
                            metric,
                            *temporality,
                            point,
                            expire_at,
                        )?);
                        for exemplar in &point.exemplars {
                            rows.exemplars
                                .extend(ExemplarRow::of(scope, metric, exemplar, expire_at)?);
                        }
                    }
                }
            }
        }
        Ok(rows)
    }

    fn numbers(
        &mut self,
        scope: Scope,
        metric: &'a Metric,
        shape: Shape,
        points: &'a [MetricPoint],
        expire_at: UnixSeconds,
    ) -> Result<(), SinkError> {
        for point in points {
            self.points
                .push(PointRow::of(scope, metric, shape, point, expire_at)?);
            for exemplar in &point.exemplars {
                self.exemplars
                    .extend(ExemplarRow::of(scope, metric, exemplar, expire_at)?);
            }
        }
        Ok(())
    }
}

impl<C: Clock> ClickhouseMetricSink<C> {
    pub fn new(client: &Client, limits: BatchLimits, retention: Duration, clock: C) -> Self {
        Self {
            points: Mutex::new(inserter(client, POINTS, limits)),
            histograms: Mutex::new(inserter(client, HISTOGRAMS, limits)),
            exemplars: Mutex::new(inserter(client, EXEMPLARS, limits)),
            catalogue: Mutex::new(inserter(client, CATALOGUE, limits)),
            clock,
            retention,
            limits,
        }
    }

    pub async fn flush(&self) -> Result<u64, SinkError> {
        let points = self.points.lock().await.force_commit().await;
        let histograms = self.histograms.lock().await.force_commit().await;
        let exemplars = self.exemplars.lock().await.force_commit().await;
        let catalogue = self.catalogue.lock().await.force_commit().await;
        let mut rows = 0;
        for committed in [points, histograms, exemplars, catalogue] {
            rows += committed.map_err(|error| refused(&error))?.rows;
        }
        Ok(rows)
    }

    pub async fn pending_rows(&self) -> u64 {
        self.points.lock().await.pending().rows
            + self.histograms.lock().await.pending().rows
            + self.exemplars.lock().await.pending().rows
            + self.catalogue.lock().await.pending().rows
    }

    pub async fn time_left(&self) -> Option<Duration> {
        [
            self.points.lock().await.time_left(),
            self.histograms.lock().await.time_left(),
            self.exemplars.lock().await.time_left(),
            self.catalogue.lock().await.time_left(),
        ]
        .into_iter()
        .flatten()
        .min()
    }
}

async fn written<R: clickhouse::RowWrite>(
    inserter: &Mutex<Inserter<R>>,
    rows: &[R::Value<'_>],
) -> Result<(), SinkError> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut inserter = inserter.lock().await;
    for row in rows {
        inserter.write(row).await.map_err(|error| refused(&error))?;
    }
    inserter.commit().await.map_err(|error| refused(&error))?;
    Ok(())
}

impl<C: Clock + Send + Sync> MetricSink for ClickhouseMetricSink<C> {
    async fn accept_metrics(&self, scope: Scope, metrics: Vec<Metric>) -> Result<(), SinkError> {
        if metrics.is_empty() {
            return Ok(());
        }
        let expire_at = expiry(self.clock.now(), self.retention, scope.retention);
        let rows = Rows::of(&metrics, scope, expire_at)?;
        written(&self.points, &rows.points).await?;
        written(&self.histograms, &rows.histograms).await?;
        written(&self.exemplars, &rows.exemplars).await?;
        written(&self.catalogue, &rows.catalogue).await?;
        Ok(())
    }
}
