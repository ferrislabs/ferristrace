use std::collections::BTreeMap;

use super::entities::HistogramPoint;
use super::errors::{BadAggregation, BadMetricKind, BadMetricQuery};
use super::value_objects::{Aggregation, MAX_GROUP_BY, MetricKind, MetricQuery, Temporality};
use crate::explore::Window;

const SECOND: u64 = 1_000_000_000;

fn a_window(from: u64, to: u64) -> Window {
    Window::new(from, to).expect("a window")
}

fn a_histogram(bounds: &[f64], counts: &[u64]) -> HistogramPoint {
    HistogramPoint {
        time_unix_nanos: 0,
        count: counts.iter().sum(),
        sum: 0.0,
        min: None,
        max: None,
        bounds: bounds.to_vec(),
        counts: counts.to_vec(),
        attributes: BTreeMap::new(),
        exemplars: Vec::new(),
    }
}

#[test]
fn every_kind_and_aggregation_reads_back_from_its_name() {
    for kind in MetricKind::ALL {
        assert_eq!(MetricKind::try_from(kind.name()), Ok(kind));
    }
    for aggregation in Aggregation::ALL {
        assert_eq!(Aggregation::try_from(aggregation.name()), Ok(aggregation));
    }
    assert_eq!(MetricKind::try_from("summary"), Err(BadMetricKind));
    assert_eq!(Aggregation::try_from("median"), Err(BadAggregation));
    assert_eq!(Temporality::Delta.name(), "delta");
    assert_eq!(Temporality::Cumulative.name(), "cumulative");
    assert_eq!(Temporality::Unspecified.name(), "unspecified");
}

#[test]
fn quantiles_apply_to_histograms_only_and_rate_to_counters_and_histograms() {
    use Aggregation::{Avg, Max, Min, P50, P95, P99, Rate, Sum};
    let allowed = |kind: MetricKind| {
        Aggregation::ALL
            .into_iter()
            .filter(|aggregation| aggregation.applies_to(kind))
            .collect::<Vec<_>>()
    };

    assert_eq!(allowed(MetricKind::Gauge), vec![Sum, Avg, Min, Max]);
    assert_eq!(allowed(MetricKind::Sum), vec![Sum, Avg, Min, Max, Rate]);
    assert_eq!(
        allowed(MetricKind::Histogram),
        vec![Avg, Rate, P50, P95, P99]
    );
    assert_eq!(P50.quantile(), Some(0.5));
    assert_eq!(P95.quantile(), Some(0.95));
    assert_eq!(P99.quantile(), Some(0.99));
    assert_eq!(Avg.quantile(), None);
}

#[test]
fn a_query_without_an_aggregation_takes_the_default_of_its_kind() {
    let query = MetricQuery::over(a_window(0, 60 * SECOND), "http.requests");

    assert_eq!(query.resolved(MetricKind::Gauge), Ok(Aggregation::Avg));
    assert_eq!(query.resolved(MetricKind::Sum), Ok(Aggregation::Rate));
    assert_eq!(query.resolved(MetricKind::Histogram), Ok(Aggregation::P95));
}

#[test]
fn an_aggregation_that_does_not_fit_the_kind_is_refused() {
    let mut query = MetricQuery::over(a_window(0, 60 * SECOND), "queue.depth");
    query.aggregation = Some(Aggregation::P99);

    assert_eq!(
        query.resolved(MetricKind::Gauge),
        Err(BadMetricQuery::Inapplicable {
            aggregation: Aggregation::P99,
            kind: MetricKind::Gauge,
        })
    );
    query.aggregation = Some(Aggregation::Rate);
    assert!(query.resolved(MetricKind::Gauge).is_err());
    assert_eq!(query.resolved(MetricKind::Sum), Ok(Aggregation::Rate));
}

#[test]
fn a_query_groups_by_a_bounded_number_of_keys() {
    let mut query = MetricQuery::over(a_window(0, 60 * SECOND), "http.requests");
    query.group_by = (0..=MAX_GROUP_BY).map(|n| format!("k{n}").into()).collect();

    assert_eq!(
        query.resolved(MetricKind::Sum),
        Err(BadMetricQuery::GroupBy { max: MAX_GROUP_BY })
    );
    query.group_by.pop();
    assert!(query.resolved(MetricKind::Sum).is_ok());
}

#[test]
fn the_step_is_a_sixtieth_of_the_window_and_never_under_a_second() {
    let hour = MetricQuery::over(a_window(0, 3600 * SECOND), "m");
    let minute = MetricQuery::over(a_window(0, 60 * SECOND), "m");
    let short = MetricQuery::over(a_window(0, 5 * SECOND), "m");
    let odd = MetricQuery::over(a_window(0, 61 * SECOND), "m");

    assert_eq!(hour.step_nanos(), 60 * SECOND);
    assert_eq!(minute.step_nanos(), SECOND);
    assert_eq!(short.step_nanos(), SECOND);
    assert_eq!(odd.step_nanos(), (61 * SECOND).div_ceil(60));
    assert_eq!(hour.slots(), 60);
    assert_eq!(short.slots(), 5);
    assert_eq!(
        hour.slot_starts().take(2).collect::<Vec<_>>(),
        vec![0, 60 * SECOND]
    );
}

#[test]
fn a_quantile_is_interpolated_inside_the_bucket_holding_its_rank() {
    let histogram = a_histogram(&[10.0, 20.0, 30.0], &[0, 10, 10, 0]);

    assert_eq!(histogram.quantile(0.5), Some(20.0));
    assert_eq!(histogram.quantile(0.25), Some(15.0));
    assert_eq!(histogram.quantile(0.75), Some(25.0));
}

#[test]
fn the_first_bucket_starts_at_the_minimum_or_zero_and_the_last_ends_at_the_maximum() {
    let mut first = a_histogram(&[10.0], &[4, 0]);
    let mut last = a_histogram(&[10.0], &[0, 4]);

    assert_eq!(first.quantile(0.5), Some(5.0));
    first.min = Some(6.0);
    assert_eq!(first.quantile(0.5), Some(8.0));
    assert_eq!(last.quantile(0.5), Some(10.0));
    last.max = Some(30.0);
    assert_eq!(last.quantile(0.5), Some(20.0));
}

#[test]
fn an_empty_histogram_has_no_quantile() {
    assert_eq!(a_histogram(&[1.0], &[0, 0]).quantile(0.5), None);
    assert_eq!(a_histogram(&[], &[]).quantile(0.5), None);
}

#[test]
fn a_histogram_without_bounds_spans_its_minimum_to_its_maximum() {
    let mut single = a_histogram(&[], &[10]);
    single.min = Some(2.0);
    single.max = Some(4.0);

    assert_eq!(single.quantile(0.5), Some(3.0));
}

#[test]
fn bucket_counts_that_do_not_match_the_bounds_have_no_quantile() {
    assert_eq!(a_histogram(&[1.0, 2.0], &[3, 3]).quantile(0.5), None);
}
