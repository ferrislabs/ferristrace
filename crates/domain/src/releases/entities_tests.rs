use super::entities::{Deploy, Traffic};

#[test]
fn an_error_rate_is_errors_over_requests() {
    let traffic = Traffic {
        requests: 200,
        errors: 50,
    };

    assert_eq!(traffic.error_rate(), Some(0.25));
}

#[test]
fn no_traffic_has_no_error_rate() {
    assert_eq!(Traffic::default().error_rate(), None);
}

#[test]
fn a_deploy_is_stamped_in_milliseconds() {
    let deploy = Deploy {
        service: "checkout".into(),
        version: "1.2.3".into(),
        environment: "prod".into(),
        at_unix_nanos: 1_700_000_000_123_456_789,
    };

    assert_eq!(deploy.at_ms(), 1_700_000_000_123);
}
