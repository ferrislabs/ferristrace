mod entities;
mod errors;
mod metric_search;
#[cfg(test)]
mod metric_search_tests;
mod ports;
mod value_objects;
#[cfg(test)]
mod value_objects_tests;

pub use entities::{Exemplar, HistogramPoint, Metric, MetricData, MetricDescriptor, MetricPoint};
pub use errors::{BadAggregation, BadMetricKind, BadMetricQuery, MetricError};
pub use metric_search::MetricSearch;
pub use ports::{MetricReader, MetricSink};
pub use value_objects::{
    Aggregation, MAX_EXEMPLARS, MAX_GROUP_BY, MIN_STEP_NANOS, MetricKind, MetricPayload,
    MetricQuery, STEPS, Series, SeriesSet, Temporality,
};
