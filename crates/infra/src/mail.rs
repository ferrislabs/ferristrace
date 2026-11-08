use std::fmt::Write as _;

use ferristrace_domain::{
    AlertMailer, AlertStatus, Culprits, DeliveryError, Dispatch, Email, EmailList, Letter,
    LetterBody, PageDispatch, PageSource, StatusMailer, UpdateKind,
};
use lettre::message::{Mailbox, header::ContentType};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailSettings {
    pub relay: String,
    pub username: String,
    pub password: String,
    pub from: String,
    pub base_url: String,
}

#[derive(Debug, thiserror::Error)]
pub enum MailError {
    #[error("the sender address or the relay is not usable")]
    Settings,
}

#[derive(Debug, Clone)]
pub struct SmtpMailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
    base_url: String,
}

impl SmtpMailer {
    pub fn connect(settings: &MailSettings) -> Result<Self, MailError> {
        let from: Mailbox = settings.from.parse().map_err(|_| MailError::Settings)?;
        let transport = AsyncSmtpTransport::<Tokio1Executor>::relay(&settings.relay)
            .map_err(|_| MailError::Settings)?
            .credentials(Credentials::new(
                settings.username.clone(),
                settings.password.clone(),
            ))
            .build();
        Ok(Self {
            transport,
            from,
            base_url: settings.base_url.trim_end_matches('/').to_owned(),
        })
    }
}

pub fn alert_subject(dispatch: &Dispatch) -> String {
    let moved = match (dispatch.from, dispatch.to) {
        (AlertStatus::Firing, AlertStatus::Ok) => "RESOLVED",
        (_, AlertStatus::Firing) => "FIRING",
        (_, AlertStatus::NoData) => "NO DATA",
        (_, AlertStatus::Pending) => "PENDING",
        (_, AlertStatus::Ok) => "OK",
    };
    format!(
        "[FerrisTrace] {moved} {}: {}",
        dispatch.severity, dispatch.name
    )
}

pub fn alert_text(dispatch: &Dispatch, link: &str) -> String {
    let mut text = format!(
        "The alert \"{}\" went from {} to {}.\n\n",
        dispatch.name, dispatch.from, dispatch.to
    );
    let _ = writeln!(text, "Severity: {}", dispatch.severity);
    match dispatch.value {
        Some(value) => {
            let _ = writeln!(text, "Value: {value}");
        }
        None => text.push_str("Value: no data\n"),
    }
    for (key, value) in dispatch.labels.pairs() {
        let _ = writeln!(text, "{key}: {value}");
    }
    let _ = writeln!(text, "At: {} ms since the epoch", dispatch.at_ms);
    if let Some(context) = &dispatch.context {
        write_context(&mut text, context);
    }
    let _ = write!(text, "\n{link}\n");
    text
}

fn write_context(text: &mut String, context: &Culprits) {
    if !context.pods().is_empty() {
        text.push_str("\nPods:\n");
        for pod in context.pods() {
            match &pod.node {
                Some(node) => {
                    let _ = writeln!(text, "  {} ({node}): {}", pod.name, pod.count);
                }
                None => {
                    let _ = writeln!(text, "  {}: {}", pod.name, pod.count);
                }
            }
        }
        if context.others() > 0 {
            let _ = writeln!(text, "  and {} more", context.others());
        }
    }
    if !context.nodes().is_empty() {
        let _ = writeln!(text, "Nodes: {}", context.nodes().join(", "));
    }
    if !context.versions().is_empty() {
        let _ = writeln!(text, "Version: {}", context.versions().join(", "));
    }
}

impl SmtpMailer {
    pub fn alert_link(&self, dispatch: &Dispatch) -> String {
        format!("{}/alerts?rule={}", self.base_url, dispatch.rule)
    }

    fn compose_alert(&self, to: &EmailList, dispatch: &Dispatch) -> Result<Message, MailError> {
        let mut builder = Message::builder().from(self.from.clone());
        for address in to.addresses() {
            let recipient: Mailbox = address.as_str().parse().map_err(|_| MailError::Settings)?;
            builder = builder.to(recipient);
        }
        builder
            .subject(alert_subject(dispatch))
            .header(ContentType::TEXT_PLAIN)
            .body(alert_text(dispatch, &self.alert_link(dispatch)))
            .map_err(|_| MailError::Settings)
    }
}

pub fn page_subject(dispatch: &PageDispatch) -> String {
    if dispatch.step == 0 {
        format!("[FerrisTrace] PAGE: {}", dispatch.headline)
    } else {
        format!(
            "[FerrisTrace] ESCALATED PAGE (step {}): {}",
            dispatch.step, dispatch.headline
        )
    }
}

pub fn page_text(dispatch: &PageDispatch, source: &str, ack: Option<&str>) -> String {
    let mut text = match dispatch.source {
        PageSource::Alert { .. } => format!(
            "You are paged because the alert \"{}\" is firing.\n\n",
            dispatch.headline
        ),
        PageSource::Incident(_) => format!(
            "You are paged for the incident \"{}\".\n\n",
            dispatch.headline
        ),
    };
    if dispatch.step > 0 {
        let _ = writeln!(
            text,
            "Nobody acknowledged it in time, so it was escalated to you (step {}).\n",
            dispatch.step
        );
    }
    let _ = writeln!(text, "Paged at: {} ms since the epoch", dispatch.at_ms);
    let _ = writeln!(text, "Open it: {source}");
    match ack {
        Some(ack) => {
            let _ = write!(
                text,
                "\nAcknowledge it, which stops the escalation:\n{ack}\n"
            );
        }
        None => text.push_str("\nAcknowledge it in the app to stop the escalation.\n"),
    }
    text
}

impl SmtpMailer {
    pub fn page_link(&self, dispatch: &PageDispatch) -> String {
        match dispatch.source {
            PageSource::Alert { rule, .. } => format!("{}/alerts?rule={rule}", self.base_url),
            PageSource::Incident(incident) => format!("{}/incidents/{incident}", self.base_url),
        }
    }

    pub fn ack_link(&self, token: &str) -> String {
        format!("{}/api/pages/ack?token={token}", self.base_url)
    }

    fn compose_page(&self, to: &Email, dispatch: &PageDispatch) -> Result<Message, MailError> {
        let recipient: Mailbox = to.as_str().parse().map_err(|_| MailError::Settings)?;
        let ack = dispatch.ack.as_deref().map(|token| self.ack_link(token));
        Message::builder()
            .from(self.from.clone())
            .to(recipient)
            .subject(page_subject(dispatch))
            .header(ContentType::TEXT_PLAIN)
            .body(page_text(
                dispatch,
                &self.page_link(dispatch),
                ack.as_deref(),
            ))
            .map_err(|_| MailError::Settings)
    }
}

impl AlertMailer for SmtpMailer {
    async fn send_page(&self, to: &Email, dispatch: &PageDispatch) -> Result<(), DeliveryError> {
        let message = self.compose_page(to, dispatch).map_err(|error| {
            tracing::error!(%error, "a page message could not be composed");
            DeliveryError::new("the page message could not be composed")
        })?;
        self.transport.send(message).await.map_err(|error| {
            tracing::warn!(%error, "a page message was not accepted by the relay");
            DeliveryError::new("the mail relay did not accept the message")
        })?;
        Ok(())
    }

    async fn send_alert(&self, to: &EmailList, dispatch: &Dispatch) -> Result<(), DeliveryError> {
        let message = self.compose_alert(to, dispatch).map_err(|error| {
            tracing::error!(%error, "an alert message could not be composed");
            DeliveryError::new("the alert message could not be composed")
        })?;
        self.transport.send(message).await.map_err(|error| {
            tracing::warn!(%error, "an alert message was not accepted by the relay");
            DeliveryError::new("the mail relay did not accept the message")
        })?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLinks {
    pub page: String,
    pub confirm: Option<String>,
    pub unsubscribe: String,
}

pub fn status_subject(letter: &Letter) -> String {
    let page = letter.page_name.as_str();
    match &letter.body {
        LetterBody::Confirm => format!("[{page} status] Confirm your subscription"),
        LetterBody::Update(bulletin) => match bulletin.kind {
            UpdateKind::Opened => format!("[{page} status] New incident: {}", bulletin.title),
            UpdateKind::Status | UpdateKind::Comment => {
                format!("[{page} status] {}: {}", bulletin.title, bulletin.status)
            }
        },
    }
}

pub fn status_text(letter: &Letter, links: &StatusLinks) -> String {
    let page = letter.page_name.as_str();
    let mut text = match &letter.body {
        LetterBody::Confirm => format!(
            "Someone, hopefully you, asked to receive the incident updates of the {page} status \
             page at this address.\n\n"
        ),
        LetterBody::Update(bulletin) => {
            let mut text = format!("{}\n\nStatus: {}\n", bulletin.title, bulletin.status);
            if bulletin.kind == UpdateKind::Comment {
                let _ = write!(text, "\n{}\n", bulletin.text);
            }
            let _ = writeln!(text, "\nAt: {} ms since the epoch", bulletin.at_ms);
            text
        }
    };
    if let Some(confirm) = &links.confirm {
        let _ = write!(
            text,
            "Confirm it within seven days by opening this link:\n{confirm}\n\n\
             If you did not ask for it, ignore this message and nothing is sent.\n"
        );
    }
    let _ = write!(
        text,
        "\nThe {page} status page: {}\nStop these emails: {}\n",
        links.page, links.unsubscribe
    );
    text
}

impl SmtpMailer {
    pub fn status_links(&self, letter: &Letter) -> StatusLinks {
        StatusLinks {
            page: format!("{}/status/{}", self.base_url, letter.slug),
            confirm: letter
                .confirm
                .as_deref()
                .map(|token| format!("{}/api/status/confirm?token={token}", self.base_url)),
            unsubscribe: format!(
                "{}/api/status/unsubscribe?token={}",
                self.base_url, letter.unsubscribe
            ),
        }
    }

    fn compose_status(&self, to: &Email, letter: &Letter) -> Result<Message, MailError> {
        let recipient: Mailbox = to.as_str().parse().map_err(|_| MailError::Settings)?;
        Message::builder()
            .from(self.from.clone())
            .to(recipient)
            .subject(status_subject(letter))
            .header(ContentType::TEXT_PLAIN)
            .body(status_text(letter, &self.status_links(letter)))
            .map_err(|_| MailError::Settings)
    }
}

impl StatusMailer for SmtpMailer {
    async fn send_status(&self, to: &Email, letter: &Letter) -> Result<(), DeliveryError> {
        let message = self.compose_status(to, letter).map_err(|error| {
            tracing::error!(%error, "a status message could not be composed");
            DeliveryError::new("the status message could not be composed")
        })?;
        self.transport.send(message).await.map_err(|error| {
            tracing::warn!(%error, "a status message was not accepted by the relay");
            DeliveryError::new("the mail relay did not accept the message")
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use ferristrace_domain::Email;

    use super::{MailSettings, SmtpMailer};

    fn mailer() -> SmtpMailer {
        SmtpMailer::connect(&MailSettings {
            relay: "mail.example.com".to_owned(),
            username: "postmaster".to_owned(),
            password: "secret".to_owned(),
            from: "FerrisTrace <no-reply@example.com>".to_owned(),
            base_url: "https://app.example.com/".to_owned(),
        })
        .expect("usable settings")
    }

    #[test]
    fn an_alert_message_names_the_change_the_value_and_the_link() {
        use std::collections::BTreeMap;

        use ferristrace_domain::{
            AlertSeverity, AlertStatus, Dispatch, EmailList, Labels, RuleId, TenantId,
        };
        use uuid::Uuid;

        let dispatch = Dispatch {
            rule: RuleId::new(Uuid::from_u128(7)),
            name: "Checkout errors".into(),
            severity: AlertSeverity::Critical,
            org: TenantId::new(Uuid::from_u128(1)),
            from: AlertStatus::Pending,
            to: AlertStatus::Firing,
            value: Some(12.5),
            labels: Labels::new(BTreeMap::from([("queue".into(), "mail".into())])),
            at_ms: 1_700_000_000_000,
            context: None,
            mentions: ferristrace_domain::Mentions::default(),
        };
        let to = EmailList::try_from("ops@example.com, oncall@example.com").expect("addresses");

        let message = mailer().compose_alert(&to, &dispatch).expect("it composes");
        let rendered = String::from_utf8(message.formatted()).expect("text");

        assert!(
            rendered.contains("[FerrisTrace] FIRING critical: Checkout errors"),
            "{rendered}"
        );
        assert!(rendered.contains("oncall@example.com"), "{rendered}");
        assert!(rendered.contains("Value: 12.5"), "{rendered}");
        assert!(rendered.contains("queue: mail"), "{rendered}");
        assert!(
            rendered.contains("https://app.example.com/alerts?rule")
                && rendered.contains("00000000-0000-0000-0000-000000000007"),
            "{rendered}"
        );
    }

    #[test]
    fn a_firing_alert_message_names_the_pods_their_nodes_and_the_version() {
        use ferristrace_domain::{
            AlertSeverity, AlertStatus, CulpritPod, Culprits, Dispatch, Labels, RuleId, TenantId,
        };
        use uuid::Uuid;

        use super::alert_text;

        let pod = |name: &str, node: Option<&str>, count| CulpritPod {
            name: name.into(),
            node: node.map(Into::into),
            count,
        };
        let mut dispatch = Dispatch {
            rule: RuleId::new(Uuid::from_u128(7)),
            name: "Checkout errors".into(),
            severity: AlertSeverity::Critical,
            org: TenantId::new(Uuid::from_u128(1)),
            from: AlertStatus::Pending,
            to: AlertStatus::Firing,
            value: Some(12.5),
            labels: Labels::none(),
            at_ms: 1_700_000_000_000,
            context: Some(Culprits::new(
                vec![
                    pod("checkout-1", Some("node-a"), 7),
                    pod("checkout-2", None, 3),
                ],
                9,
                vec!["node-a".into(), "node-b".into()],
                vec!["1.4.2".into()],
                &Labels::none(),
            )),
            mentions: ferristrace_domain::Mentions::default(),
        };

        let text = alert_text(&dispatch, "https://app.example.com/alerts");

        assert!(text.contains("  checkout-1 (node-a): 7\n"), "{text}");
        assert!(text.contains("  checkout-2: 3\n"), "{text}");
        assert!(text.contains("  and 7 more\n"), "{text}");
        assert!(text.contains("Nodes: node-a, node-b\n"), "{text}");
        assert!(text.contains("Version: 1.4.2\n"), "{text}");
        dispatch.context = None;
        let bare = alert_text(&dispatch, "https://app.example.com/alerts");
        assert!(
            !bare.contains("Pods:") && !bare.contains("Version:"),
            "{bare}"
        );
    }

    #[test]
    fn a_page_message_names_the_incident_its_step_and_carries_both_links() {
        use ferristrace_domain::{IncidentId, PageDispatch, PageId, PageSource, TenantId};
        use uuid::Uuid;

        let dispatch = PageDispatch {
            page: PageId::new(Uuid::from_u128(5)),
            org: TenantId::new(Uuid::from_u128(1)),
            source: PageSource::Incident(IncidentId::new(Uuid::from_u128(9))),
            headline: "Checkout down".into(),
            step: 1,
            at_ms: 1_700_000_000_000,
            ack: Some("v1.abc.def".into()),
        };

        let message = mailer()
            .compose_page(
                &Email::try_from("ada@example.com").expect("an address"),
                &dispatch,
            )
            .expect("it composes");
        let rendered = String::from_utf8(message.formatted()).expect("text");

        assert!(
            rendered.contains("[FerrisTrace] ESCALATED PAGE (step 1): Checkout down"),
            "{rendered}"
        );
        assert!(rendered.contains("ada@example.com"), "{rendered}");
        let held = mailer();
        let text = super::page_text(
            &dispatch,
            &held.page_link(&dispatch),
            dispatch
                .ack
                .as_deref()
                .map(|token| held.ack_link(token))
                .as_deref(),
        );
        assert!(
            text.contains("https://app.example.com/incidents/00000000-0000-0000-0000-000000000009"),
            "{text}"
        );
        assert!(
            text.contains("https://app.example.com/api/pages/ack?token=v1.abc.def"),
            "{text}"
        );
        assert!(text.contains("escalated to you (step 1)"), "{text}");
    }

    #[test]
    fn an_alert_page_without_a_key_points_to_the_app_to_acknowledge() {
        use ferristrace_domain::{EventId, PageDispatch, PageId, PageSource, RuleId, TenantId};
        use uuid::Uuid;

        let dispatch = PageDispatch {
            page: PageId::new(Uuid::from_u128(5)),
            org: TenantId::new(Uuid::from_u128(1)),
            source: PageSource::Alert {
                rule: RuleId::new(Uuid::from_u128(7)),
                event: EventId::new(Uuid::from_u128(8)),
            },
            headline: "Checkout errors".into(),
            step: 0,
            at_ms: 1,
            ack: None,
        };

        let text = super::page_text(&dispatch, &mailer().page_link(&dispatch), None);

        assert_eq!(
            super::page_subject(&dispatch),
            "[FerrisTrace] PAGE: Checkout errors"
        );
        assert!(
            text.contains("the alert \"Checkout errors\" is firing"),
            "{text}"
        );
        assert!(
            text.contains("/alerts?rule=00000000-0000-0000-0000-000000000007"),
            "{text}"
        );
        assert!(text.contains("in the app"), "{text}");
    }

    #[test]
    fn a_status_confirmation_carries_both_signed_links_under_the_configured_host() {
        use ferristrace_domain::{Letter, LetterBody, PageName, Slug};

        let letter = Letter {
            page_name: PageName::try_from("Acme").expect("a name"),
            slug: Slug::try_from("acme").expect("a slug"),
            body: LetterBody::Confirm,
            confirm: Some("v1.c.abc.def".into()),
            unsubscribe: "v1.u.abc.ghi".into(),
        };

        let message = mailer()
            .compose_status(
                &Email::try_from("ada@example.com").expect("an address"),
                &letter,
            )
            .expect("it composes");
        let rendered = String::from_utf8(message.formatted()).expect("text");
        let text = super::status_text(&letter, &mailer().status_links(&letter));

        assert!(rendered.contains("ada@example.com"), "{rendered}");
        assert_eq!(
            super::status_subject(&letter),
            "[Acme status] Confirm your subscription"
        );
        assert!(
            text.contains("https://app.example.com/api/status/confirm?token=v1.c.abc.def"),
            "{text}"
        );
        assert!(
            text.contains("https://app.example.com/api/status/unsubscribe?token=v1.u.abc.ghi"),
            "{text}"
        );
    }

    #[test]
    fn a_status_update_names_the_incident_its_status_and_the_page() {
        use ferristrace_domain::{
            Bulletin, IncidentStatus, IncidentTitle, Letter, LetterBody, PageName, Slug, UpdateKind,
        };

        let letter = Letter {
            page_name: PageName::try_from("Acme").expect("a name"),
            slug: Slug::try_from("acme").expect("a slug"),
            body: LetterBody::Update(Bulletin {
                title: IncidentTitle::try_from("Payments fail").expect("a title"),
                kind: UpdateKind::Comment,
                status: IncidentStatus::Identified,
                text: "A provider is down; we are switching.".into(),
                at_ms: 1_700_000_000_000,
            }),
            confirm: None,
            unsubscribe: "v1.u.abc.ghi".into(),
        };

        let text = super::status_text(&letter, &mailer().status_links(&letter));

        assert_eq!(
            super::status_subject(&letter),
            "[Acme status] Payments fail: identified"
        );
        assert!(
            text.contains("A provider is down; we are switching."),
            "{text}"
        );
        assert!(
            text.contains("https://app.example.com/status/acme"),
            "{text}"
        );
        assert!(
            text.contains("/api/status/unsubscribe?token=v1.u.abc.ghi"),
            "{text}"
        );
        assert!(!text.contains("/api/status/confirm"), "{text}");
    }

    #[test]
    fn a_misshapen_sender_is_refused_before_anything_is_sent() {
        let refused = SmtpMailer::connect(&MailSettings {
            relay: "mail.example.com".to_owned(),
            username: String::new(),
            password: String::new(),
            from: "not an address".to_owned(),
            base_url: "https://app.example.com".to_owned(),
        });

        assert!(refused.is_err());
    }
}
