use std::sync::atomic::{AtomicBool, Ordering};

use ferristrace_domain::{
    Exemplar, HistogramPoint, Metric, MetricData, MetricPoint, SpanId, Temporality, TraceId,
};
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::tonic::metrics::v1::exemplar::Value as ExemplarValue;
use opentelemetry_proto::tonic::metrics::v1::metric::Data;
use opentelemetry_proto::tonic::metrics::v1::number_data_point::Value as NumberValue;
use opentelemetry_proto::tonic::metrics::v1::{
    AggregationTemporality, Exemplar as ProtoExemplar, HistogramDataPoint, Metric as ProtoMetric,
    NumberDataPoint,
};
use prost::Message;

use crate::attributes::{self, Attributes};
use crate::logs::optional_id;
use crate::traces::{DecodeError, UNKNOWN_SERVICE};

const SERVICE_NAME: &str = "service.name";
const NO_RECORDED_VALUE: u32 = 1;

static SKIPPED_REPORTED: AtomicBool = AtomicBool::new(false);

pub fn decode_metrics(payload: &[u8]) -> Result<Vec<Metric>, DecodeError> {
    metrics_of(ExportMetricsServiceRequest::decode(payload)?)
}

pub fn decode_metrics_json(payload: &[u8]) -> Result<Vec<Metric>, DecodeError> {
    metrics_of(serde_json::from_slice(payload)?)
}

pub fn metrics_of(request: ExportMetricsServiceRequest) -> Result<Vec<Metric>, DecodeError> {
    let mut metrics = Vec::new();
    for resource_metrics in request.resource_metrics {
        let pairs = resource_metrics
            .resource
            .map(|resource| resource.attributes)
            .unwrap_or_default();
        let service =
            attributes::lookup(&pairs, SERVICE_NAME).unwrap_or_else(|| UNKNOWN_SERVICE.into());
        let resource = attributes::collect(pairs);
        for scope_metrics in resource_metrics.scope_metrics {
            for metric in scope_metrics.metrics {
                if let Some(converted) = convert(metric, &service, &resource)? {
                    metrics.push(converted);
                }
            }
        }
    }
    Ok(metrics)
}

#[cfg(test)]
pub(crate) fn skipped_was_reported() -> bool {
    SKIPPED_REPORTED.load(Ordering::Relaxed)
}

fn report_skipped(name: &str) {
    if !SKIPPED_REPORTED.swap(true, Ordering::Relaxed) {
        tracing::warn!(
            metric = name,
            "exponential histograms and summaries are accepted and counted but not stored; \
             this is reported once per process"
        );
    }
}

fn convert(
    metric: ProtoMetric,
    service: &str,
    resource: &Attributes,
) -> Result<Option<Metric>, DecodeError> {
    let ProtoMetric {
        name,
        description,
        unit,
        data,
        ..
    } = metric;
    let data = match data {
        None => return Ok(None),
        Some(Data::ExponentialHistogram(_) | Data::Summary(_)) => {
            report_skipped(&name);
            return Ok(None);
        }
        Some(Data::Gauge(gauge)) => MetricData::Gauge(numbers(gauge.data_points, resource)?),
        Some(Data::Sum(sum)) => MetricData::Sum {
            monotonic: sum.is_monotonic,
            temporality: temporality_of(sum.aggregation_temporality),
            points: numbers(sum.data_points, resource)?,
        },
        Some(Data::Histogram(histogram)) => MetricData::Histogram {
            temporality: temporality_of(histogram.aggregation_temporality),
            points: histogram
                .data_points
                .into_iter()
                .filter(|point| point.flags & NO_RECORDED_VALUE == 0)
                .map(|point| histogram_point(point, resource))
                .collect::<Result<_, _>>()?,
        },
    };
    if data.last_seen_unix_nanos().is_none() {
        return Ok(None);
    }
    Ok(Some(Metric {
        name: name.into_boxed_str(),
        unit: unit.into_boxed_str(),
        description: description.into_boxed_str(),
        service_name: service.into(),
        data,
    }))
}

fn temporality_of(number: i32) -> Temporality {
    match AggregationTemporality::try_from(number) {
        Ok(AggregationTemporality::Delta) => Temporality::Delta,
        Ok(AggregationTemporality::Cumulative) => Temporality::Cumulative,
        Ok(AggregationTemporality::Unspecified) | Err(_) => Temporality::Unspecified,
    }
}

fn numbers(
    points: Vec<NumberDataPoint>,
    resource: &Attributes,
) -> Result<Vec<MetricPoint>, DecodeError> {
    let mut kept = Vec::with_capacity(points.len());
    for point in points {
        if point.flags & NO_RECORDED_VALUE != 0 {
            continue;
        }
        let Some(value) = point.value else {
            continue;
        };
        kept.push(MetricPoint {
            time_unix_nanos: point.time_unix_nano,
            value: number_of(value),
            attributes: attributes::merged(resource, point.attributes),
            exemplars: exemplars(point.exemplars)?,
        });
    }
    Ok(kept)
}

#[allow(clippy::cast_precision_loss)]
fn number_of(value: NumberValue) -> f64 {
    match value {
        NumberValue::AsDouble(number) => number,
        NumberValue::AsInt(number) => number as f64,
    }
}

fn histogram_point(
    point: HistogramDataPoint,
    resource: &Attributes,
) -> Result<HistogramPoint, DecodeError> {
    Ok(HistogramPoint {
        time_unix_nanos: point.time_unix_nano,
        count: point.count,
        sum: point.sum.unwrap_or_default(),
        min: point.min,
        max: point.max,
        bounds: point.explicit_bounds,
        counts: point.bucket_counts,
        attributes: attributes::merged(resource, point.attributes),
        exemplars: exemplars(point.exemplars)?,
    })
}

#[allow(clippy::cast_precision_loss)]
fn exemplars(exemplars: Vec<ProtoExemplar>) -> Result<Vec<Exemplar>, DecodeError> {
    let mut kept = Vec::with_capacity(exemplars.len());
    for exemplar in exemplars {
        let Some(trace_id) = optional_id(&exemplar.trace_id, |bytes| TraceId::try_from(bytes))?
        else {
            continue;
        };
        kept.push(Exemplar {
            time_unix_nanos: exemplar.time_unix_nano,
            value: match exemplar.value {
                Some(ExemplarValue::AsDouble(number)) => number,
                Some(ExemplarValue::AsInt(number)) => number as f64,
                None => 0.0,
            },
            trace_id: Some(trace_id),
            span_id: optional_id(&exemplar.span_id, |bytes| SpanId::try_from(bytes))?,
            attributes: attributes::collect(exemplar.filtered_attributes),
        });
    }
    Ok(kept)
}
