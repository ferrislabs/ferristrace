use super::value_objects::{Aggregation, MetricKind};
use crate::explore::ExploreError;
use crate::store::StoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a metric kind is one of gauge, sum or histogram")]
pub struct BadMetricKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("an aggregation is one of sum, avg, min, max, rate, p50, p95 or p99")]
pub struct BadAggregation;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadMetricQuery {
    #[error("{aggregation} does not apply to a {kind}")]
    Inapplicable {
        aggregation: Aggregation,
        kind: MetricKind,
    },
    #[error("a series groups by at most {max} attribute keys")]
    GroupBy { max: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MetricError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("this reader holds no grant on that organisation")]
    NotYours,
    #[error("this agent holds no scope that allows this")]
    OutOfScope,
    #[error("a store could not be reached")]
    StoreUnreachable,
    #[error("no metric of that name was seen")]
    Unknown,
    #[error(transparent)]
    Query(#[from] BadMetricQuery),
}

impl From<ExploreError> for MetricError {
    fn from(error: ExploreError) -> Self {
        match error {
            ExploreError::NoSession => Self::NoSession,
            ExploreError::NotYours => Self::NotYours,
            ExploreError::OutOfScope => Self::OutOfScope,
            ExploreError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<StoreError> for MetricError {
    fn from(error: StoreError) -> Self {
        ExploreError::from(error).into()
    }
}
