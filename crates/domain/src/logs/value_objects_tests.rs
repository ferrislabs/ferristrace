use super::errors::BadLogQuery;
use super::value_objects::{Buckets, LogBucket, LogQuery};
use crate::explore::Window;
use crate::telemetry::Severity;

fn a_window(from: u64, to: u64) -> Window {
    Window::new(from, to).expect("a window")
}

#[test]
fn a_query_asks_for_a_hundred_records_until_told_otherwise() {
    let query = LogQuery::over(a_window(0, 10));

    assert_eq!(query.limit(), 100);
    assert!(query.services.is_empty());
    assert!(query.severities.is_empty());
    assert_eq!(query.text, None);
    assert!(query.attributes.is_empty());
    assert_eq!(query.trace, None);
    assert_eq!(query.project, None);
    assert_eq!(query.before, None);
}

#[test]
fn a_limit_is_held_between_one_and_five_hundred() {
    let query = LogQuery::over(a_window(0, 10));

    assert_eq!(
        query.clone().at_most(0),
        Err(BadLogQuery::Limit { max: 500 })
    );
    assert_eq!(
        query.clone().at_most(501),
        Err(BadLogQuery::Limit { max: 500 })
    );
    assert_eq!(query.clone().at_most(1).map(|q| q.limit()), Ok(1));
    assert_eq!(query.at_most(500).map(|q| q.limit()), Ok(500));
}

#[test]
fn buckets_are_held_between_one_and_two_hundred_and_default_to_sixty() {
    assert_eq!(Buckets::default().count(), 60);
    assert_eq!(Buckets::try_from(0), Err(BadLogQuery::Buckets { max: 200 }));
    assert_eq!(
        Buckets::try_from(201),
        Err(BadLogQuery::Buckets { max: 200 })
    );
    assert_eq!(Buckets::try_from(200).map(Buckets::count), Ok(200));
}

#[test]
fn buckets_split_a_window_into_equal_slots_covering_it() {
    let buckets = Buckets::try_from(4).expect("four");
    let window = a_window(1_000, 1_010);

    assert_eq!(buckets.width_over(window), 3);
    let laid = buckets.laid_over(window);
    assert_eq!(
        laid.iter().map(|b| b.start_unix_nanos).collect::<Vec<_>>(),
        vec![1_000, 1_003, 1_006, 1_009]
    );
    assert!(laid.iter().all(|b| b.total() == 0));
}

#[test]
fn a_window_shorter_than_the_buckets_asked_gets_one_bucket_per_nanosecond() {
    let buckets = Buckets::try_from(60).expect("sixty");
    let window = a_window(0, 3);

    assert_eq!(buckets.width_over(window), 1);
    assert_eq!(buckets.laid_over(window).len(), 3);
}

#[test]
fn a_bucket_counts_each_severity_apart() {
    let mut bucket = LogBucket::empty(0);

    bucket.add(Severity::Error, 2);
    bucket.add(Severity::Info, 5);
    bucket.add(Severity::Error, 1);

    assert_eq!(bucket.count(Severity::Error), 3);
    assert_eq!(bucket.count(Severity::Info), 5);
    assert_eq!(bucket.count(Severity::Fatal), 0);
    assert_eq!(bucket.total(), 8);
}
