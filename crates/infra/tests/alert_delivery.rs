use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use ferristrace_domain::testing::{AlertLedger, Couriered, IncidentLedger};
use ferristrace_domain::{
    AccountId, AlertChannels, AlertRules, AlertSeverity, AlertStatus, ChannelId, ChannelKind,
    ChannelName, Comparison, Condition, Courier, Definition, DefinitionParts, DeliveryStatus,
    Destination, Drained, Evaluator, Followers, Labels, NoDataPolicy, Observation, ProbeError,
    ProjectId, RuleDraft, RuleName, Signal, SignalProbe, TenantId, TraceMeasure, TraceSignal,
    UnixSeconds, WebhookSecret, Window,
};
use ferristrace_infra::{HttpWebhooks, SIGNATURE_HEADER, WebhookSettings, signature};
use uuid::Uuid;

const START: u64 = 1_700_000_040;

#[derive(Debug, Default)]
struct Received {
    calls: Mutex<Vec<(Option<String>, Bytes)>>,
    failing: Mutex<bool>,
}

async fn receive(
    State(received): State<Arc<Received>>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    let signed = headers
        .get(SIGNATURE_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    received
        .calls
        .lock()
        .expect("the lock")
        .push((signed, body));
    if *received.failing.lock().expect("the lock") {
        StatusCode::INTERNAL_SERVER_ERROR
    } else {
        StatusCode::NO_CONTENT
    }
}

async fn a_receiver() -> (String, Arc<Received>) {
    let received = Arc::new(Received::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a port");
    let address = listener.local_addr().expect("an address");
    let app = Router::new()
        .route("/hook", post(receive))
        .with_state(Arc::clone(&received));
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serving");
    });
    (format!("http://{address}/hook"), received)
}

#[derive(Default)]
struct Scripted(Mutex<VecDeque<Option<f64>>>);

impl SignalProbe for &Scripted {
    async fn observe(
        &self,
        _org: TenantId,
        _project: Option<ProjectId>,
        _signal: &Signal,
        _window: Window,
    ) -> Result<Vec<Observation>, ProbeError> {
        let value = self.0.lock().expect("the lock").pop_front().flatten();
        Ok(vec![Observation {
            labels: Labels::none(),
            value,
        }])
    }
}

fn org() -> TenantId {
    TenantId::new(Uuid::from_u128(1))
}

fn by() -> AccountId {
    AccountId::new(Uuid::from_u128(9))
}

async fn a_channel(
    ledger: &AlertLedger,
    name: &str,
    kind: ChannelKind,
    target: &str,
    secret: Option<&WebhookSecret>,
) -> ChannelId {
    AlertChannels::create(
        ledger,
        org(),
        &ChannelName::try_from(name).expect("a name"),
        &Destination::parse(kind, target).expect("a target"),
        secret,
        by(),
    )
    .await
    .expect("kept")
    .expect("fresh")
    .id
}

async fn a_rule(ledger: &AlertLedger, channels: Vec<ChannelId>) {
    AlertRules::create(
        ledger,
        org(),
        &RuleDraft {
            name: RuleName::try_from("Checkout errors").expect("a name"),
            project: None,
            enabled: true,
            definition: Definition::new(DefinitionParts {
                signal: Signal::Traces(TraceSignal {
                    service: Some("checkout".into()),
                    operation: None,
                    environment: None,
                    measure: TraceMeasure::ErrorRate,
                }),
                condition: Condition::new(Comparison::Above, 5.0).expect("a threshold"),
                window_secs: 300,
                pending_secs: 60,
                severity: AlertSeverity::Critical,
                no_data: NoDataPolicy::Ok,
                channels,
            })
            .expect("a definition"),
        },
        by(),
    )
    .await
    .expect("kept")
    .expect("fresh");
}

fn webhooks() -> HttpWebhooks {
    HttpWebhooks::new(WebhookSettings {
        allow_private: true,
        base_url: "https://ferristrace.example.com/".to_owned(),
        ..WebhookSettings::default()
    })
}

#[tokio::test]
async fn spec_l2_2_each_firing_and_recovery_sends_one_email_and_one_signed_webhook_per_channel() {
    let (url, received) = a_receiver().await;
    let ledger = AlertLedger::sealed();
    let secret = WebhookSecret::generate();
    let mail = a_channel(&ledger, "ops", ChannelKind::Email, "ops@example.com", None).await;
    let hook = a_channel(&ledger, "pager", ChannelKind::Webhook, &url, Some(&secret)).await;
    a_rule(&ledger, vec![mail, hook]).await;
    let probe = Scripted(Mutex::new(VecDeque::from([
        Some(1.0),
        Some(9.0),
        Some(9.0),
        Some(9.0),
        Some(2.0),
    ])));
    let (mailer, hooks) = (Couriered::default(), webhooks());

    for minute in 0..5 {
        let now = UnixSeconds::new(START + minute * 60);
        Evaluator::new(
            ledger.clone(),
            ledger.clone(),
            ledger.clone(),
            ledger.clone(),
            ledger.clone(),
            &probe,
            Followers::new(IncidentLedger::default(), IncidentLedger::default()),
        )
        .tick(now)
        .await
        .expect("a tick");
        Courier::new(ledger.clone(), ledger.clone(), &mailer, &hooks)
            .drain(now)
            .await
            .expect("a drain");
    }

    let emailed: Vec<(AlertStatus, AlertStatus)> = mailer
        .sent()
        .iter()
        .map(|(to, dispatch)| {
            assert_eq!(&**to, "ops@example.com");
            (dispatch.from, dispatch.to)
        })
        .collect();
    assert_eq!(
        emailed,
        vec![
            (AlertStatus::Pending, AlertStatus::Firing),
            (AlertStatus::Firing, AlertStatus::Ok),
        ]
    );
    let calls = received.calls.lock().expect("the lock");
    assert_eq!(calls.len(), 2, "one webhook per change");
    let mut moved = Vec::new();
    for (signed, body) in calls.iter() {
        assert_eq!(
            signed.as_deref(),
            Some(signature(&secret, body).as_str()),
            "every webhook is signed with the channel secret"
        );
        let payload: serde_json::Value = serde_json::from_slice(body).expect("json");
        assert_eq!(payload["alert"]["name"], "Checkout errors");
        assert_eq!(payload["alert"]["severity"], "critical");
        assert_eq!(payload["org"], org().to_string());
        assert!(
            payload["url_to_alert"].as_str().is_some_and(
                |link| link.starts_with("https://ferristrace.example.com/alerts?rule=")
            ),
            "{payload}"
        );
        assert!(payload["at_ms"].is_u64());
        moved.push((
            payload["from"].as_str().unwrap_or_default().to_owned(),
            payload["to"].as_str().unwrap_or_default().to_owned(),
            payload["value"].as_f64(),
        ));
    }
    assert_eq!(
        moved,
        vec![
            ("pending".to_owned(), "firing".to_owned(), Some(9.0)),
            ("firing".to_owned(), "ok".to_owned(), Some(2.0)),
        ]
    );
    assert!(
        ledger
            .notifications()
            .iter()
            .all(|notification| notification.status == DeliveryStatus::Sent)
    );
}

#[tokio::test]
async fn spec_l2_2_a_failing_webhook_is_retried_then_recorded_as_abandoned() {
    let (url, received) = a_receiver().await;
    *received.failing.lock().expect("the lock") = true;
    let ledger = AlertLedger::sealed();
    let secret = WebhookSecret::generate();
    let hook = a_channel(&ledger, "pager", ChannelKind::Webhook, &url, Some(&secret)).await;
    a_rule(&ledger, vec![hook]).await;
    let probe = Scripted(Mutex::new(VecDeque::from([Some(9.0), Some(9.0)])));
    let (mailer, hooks) = (Couriered::default(), webhooks());
    for minute in 0..2 {
        Evaluator::new(
            ledger.clone(),
            ledger.clone(),
            ledger.clone(),
            ledger.clone(),
            ledger.clone(),
            &probe,
            Followers::new(IncidentLedger::default(), IncidentLedger::default()),
        )
        .tick(UnixSeconds::new(START + minute * 60))
        .await
        .expect("a tick");
    }

    let mut at = START + 60;
    let mut drained = Vec::new();
    for wait in [0, 60, 300, 900, 3_600] {
        at += wait;
        drained.push(
            Courier::new(ledger.clone(), ledger.clone(), &mailer, &hooks)
                .drain(UnixSeconds::new(at))
                .await
                .expect("a drain"),
        );
    }

    assert_eq!(
        drained,
        vec![
            Drained::Delivered {
                sent: 0,
                retried: 1,
                abandoned: 0
            },
            Drained::Delivered {
                sent: 0,
                retried: 1,
                abandoned: 0
            },
            Drained::Delivered {
                sent: 0,
                retried: 1,
                abandoned: 0
            },
            Drained::Delivered {
                sent: 0,
                retried: 1,
                abandoned: 0
            },
            Drained::Delivered {
                sent: 0,
                retried: 0,
                abandoned: 1
            },
        ]
    );
    assert_eq!(received.calls.lock().expect("the lock").len(), 5);
    let notification = &ledger.notifications()[0];
    assert_eq!(notification.status, DeliveryStatus::Abandoned);
    assert_eq!(notification.attempt, 5);
    assert_eq!(
        notification.last_error.as_deref(),
        Some("the webhook answered 500")
    );
}

#[tokio::test]
async fn a_guarded_deployment_never_reaches_a_loopback_webhook() {
    let (url, received) = a_receiver().await;
    let guarded = HttpWebhooks::new(WebhookSettings::default());
    let dispatch = ferristrace_domain::Dispatch {
        rule: ferristrace_domain::RuleId::fresh(),
        name: "x".into(),
        severity: AlertSeverity::Info,
        org: org(),
        from: AlertStatus::Ok,
        to: AlertStatus::Firing,
        value: None,
        labels: Labels::none(),
        at_ms: 0,
    };

    let refused = ferristrace_domain::WebhookSender::send(
        &guarded,
        &ferristrace_domain::WebhookUrl::try_from(url.as_str()).expect("a url"),
        &WebhookSecret::generate(),
        &dispatch,
    )
    .await;

    assert!(refused.is_err());
    assert!(received.calls.lock().expect("the lock").is_empty());
}
