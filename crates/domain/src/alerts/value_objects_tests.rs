use std::collections::BTreeMap;

use uuid::Uuid;

use super::errors::{BadChannel, BadRule};
use super::value_objects::{
    AlertSeverity, AlertStatus, ChannelId, ChannelKind, Comparison, Condition, Definition,
    DefinitionParts, Destination, EmailList, Labels, MetricSignal, NoDataPolicy, RuleName, Signal,
    TraceMeasure, TraceSignal, WebhookSecret, WebhookUrl,
};
use crate::metrics::Aggregation;

fn traces(service: Option<&str>, operation: Option<&str>) -> Signal {
    Signal::Traces(TraceSignal {
        service: service.map(Into::into),
        operation: operation.map(Into::into),
        environment: None,
        measure: TraceMeasure::ErrorRate,
        placement: crate::placement::Placement::default(),
    })
}

fn parts(signal: Signal, window_secs: u64, pending_secs: u64) -> DefinitionParts {
    DefinitionParts {
        signal,
        condition: Condition::new(Comparison::Above, 5.0).expect("a threshold"),
        window_secs,
        pending_secs,
        severity: AlertSeverity::Critical,
        no_data: NoDataPolicy::Ok,
        channels: Vec::new(),
        page: None,
    }
}

#[test]
fn a_window_lasts_a_minute_to_a_day() {
    for window in [60, 300, 86_400] {
        assert!(Definition::new(parts(traces(None, None), window, 0)).is_ok());
    }
    for window in [0, 59, 86_401] {
        assert_eq!(
            Definition::new(parts(traces(None, None), window, 0)).err(),
            Some(BadRule::Window {
                min: 60,
                max: 86_400
            })
        );
    }
}

#[test]
fn a_pending_duration_lasts_at_most_an_hour() {
    assert!(Definition::new(parts(traces(None, None), 300, 3_600)).is_ok());
    assert_eq!(
        Definition::new(parts(traces(None, None), 300, 3_601)).err(),
        Some(BadRule::Pending { max: 3_600 })
    );
}

#[test]
fn a_threshold_is_finite() {
    assert_eq!(
        Condition::new(Comparison::Above, f64::NAN).err(),
        Some(BadRule::Threshold)
    );
    assert_eq!(
        Condition::new(Comparison::Below, f64::INFINITY).err(),
        Some(BadRule::Threshold)
    );
}

#[test]
fn an_operation_is_named_within_its_service() {
    assert_eq!(
        Definition::new(parts(traces(None, Some("GET /")), 300, 0)).err(),
        Some(BadRule::OperationWithoutService)
    );
    assert!(Definition::new(parts(traces(Some("checkout"), Some("GET /")), 300, 0)).is_ok());
}

#[test]
fn a_metric_rule_names_its_metric_and_at_most_eight_keys() {
    let metric = |name: &str, keys: usize| {
        Signal::Metrics(MetricSignal {
            name: name.into(),
            aggregation: Aggregation::Avg,
            group_by: (0..keys).map(|key| format!("k{key}").into()).collect(),
            filter: BTreeMap::from([("k0".into(), "v".into())]),
        })
    };

    assert_eq!(
        Definition::new(parts(metric(" ", 1), 300, 0)).err(),
        Some(BadRule::MetricName)
    );
    assert!(Definition::new(parts(metric("cpu", 8), 300, 0)).is_ok());
    assert_eq!(
        Definition::new(parts(metric("cpu", 9), 300, 0)).err(),
        Some(BadRule::GroupBy { max: 8 })
    );
}

#[test]
fn a_channel_named_twice_is_kept_once() {
    let channel = ChannelId::new(Uuid::from_u128(4));
    let mut asked = parts(traces(None, None), 300, 0);
    asked.channels = vec![channel, channel];

    let definition = Definition::new(asked).expect("a definition");

    assert_eq!(definition.channels(), &[channel]);
}

#[test]
fn a_rule_name_is_trimmed_and_bounded() {
    assert_eq!(
        RuleName::try_from("  Checkout errors ")
            .expect("a name")
            .as_str(),
        "Checkout errors"
    );
    assert!(RuleName::try_from("   ").is_err());
    assert!(RuleName::try_from("x".repeat(129).as_str()).is_err());
}

#[test]
fn every_spelling_reads_back() {
    for status in AlertStatus::ALL {
        assert_eq!(AlertStatus::parse(status.as_str()), Some(*status));
    }
    for op in Comparison::ALL {
        assert_eq!(Comparison::parse(op.as_str()), Some(*op));
    }
    assert_eq!(AlertStatus::NoData.as_str(), "no_data");
    assert_eq!(Comparison::AtLeast.as_str(), ">=");
    assert_eq!(
        TraceMeasure::parse("error_rate"),
        Some(TraceMeasure::ErrorRate)
    );
    assert_eq!(AlertSeverity::parse("sev1"), None);
}

#[test]
fn a_label_key_tells_apart_sets_that_join_alike() {
    let one = Labels::new(BTreeMap::from([("a".into(), "b:c".into())]));
    let other = Labels::new(BTreeMap::from([
        ("a".into(), "b".into()),
        ("c".into(), String::new().into()),
    ]));

    assert_ne!(one.key(), other.key());
    assert_eq!(Labels::none().key(), "");
}

#[test]
fn an_email_channel_lists_its_addresses() {
    let list = EmailList::try_from("Ops@Example.com, oncall@example.com;ops@example.com")
        .expect("addresses");

    assert_eq!(list.target(), "ops@example.com, oncall@example.com");
    assert_eq!(
        EmailList::try_from(" , ").err(),
        Some(BadChannel::NoAddress)
    );
    assert_eq!(
        EmailList::try_from("not an address").err(),
        Some(BadChannel::Address)
    );
}

#[test]
fn a_webhook_is_an_http_url_with_a_host() {
    let url = WebhookUrl::try_from("https://hooks.example.com:8443/in?x=1").expect("a url");
    assert_eq!(url.host(), "hooks.example.com");
    assert!(url.is_https());
    assert_eq!(
        WebhookUrl::try_from("http://[::1]:80/x")
            .expect("a url")
            .host(),
        "::1"
    );
    for refused in [
        "ftp://example.com",
        "https://",
        "https://user:pass@example.com/",
        "https://exa mple.com",
        "example.com",
    ] {
        assert_eq!(
            WebhookUrl::try_from(refused).err(),
            Some(BadChannel::Url),
            "{refused}"
        );
    }
}

#[test]
fn a_destination_is_parsed_by_its_kind() {
    let hook =
        Destination::parse(ChannelKind::Webhook, "https://example.com/hook").expect("a hook");

    assert_eq!(hook.kind(), ChannelKind::Webhook);
    assert_eq!(hook.target(), "https://example.com/hook");
}

#[test]
fn a_secret_is_thirty_two_random_bytes_in_hex_and_never_printed() {
    let one = WebhookSecret::generate();
    let other = WebhookSecret::generate();

    assert_eq!(one.exposed().len(), 64);
    assert!(one.exposed().bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_ne!(one, other);
    assert_eq!(format!("{one:?}"), "WebhookSecret(redacted)");
}
