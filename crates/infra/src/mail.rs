use std::fmt::Write as _;

use ferristrace_domain::{
    AlertMailer, AlertStatus, DeliveryError, Dispatch, Email, EmailList, Mailer, ResetError,
    ResetToken,
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

    pub fn reset_link(&self, token: &ResetToken) -> String {
        format!("{}/reset?token={}", self.base_url, token.exposed())
    }

    fn compose(&self, to: &Email, token: &ResetToken) -> Result<Message, MailError> {
        let recipient: Mailbox = to.as_str().parse().map_err(|_| MailError::Settings)?;
        Message::builder()
            .from(self.from.clone())
            .to(recipient)
            .subject("Reset your ferristrace password")
            .header(ContentType::TEXT_PLAIN)
            .body(format!(
                "Open this link to choose a new password. It works once and expires within the hour.\n\n{}\n\nIf you did not ask for this, nothing has changed and you can ignore it.\n",
                self.reset_link(token)
            ))
            .map_err(|_| MailError::Settings)
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
    let _ = write!(text, "\n{link}\n");
    text
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

impl AlertMailer for SmtpMailer {
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

impl Mailer for SmtpMailer {
    async fn send_password_reset(&self, to: &Email, token: &ResetToken) -> Result<(), ResetError> {
        let message = self.compose(to, token).map_err(|error| {
            tracing::error!(%error, "a reset message could not be composed");
            ResetError::Undeliverable
        })?;
        self.transport.send(message).await.map_err(|error| {
            tracing::error!(%error, "a reset message was not accepted by the relay");
            ResetError::Undeliverable
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use ferristrace_domain::{Email, ResetToken};

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
    fn a_link_carries_the_token_under_the_configured_host() {
        let token = ResetToken::from_plaintext("ft_pr_abc");

        assert_eq!(
            mailer().reset_link(&token),
            "https://app.example.com/reset?token=ft_pr_abc",
            "the trailing slash of the base url is not doubled"
        );
    }

    #[test]
    fn a_composed_message_names_both_parties_and_carries_the_link() {
        let token = ResetToken::from_plaintext("ft_pr_abc");

        let message = mailer()
            .compose(
                &Email::try_from("person@example.com").expect("an address"),
                &token,
            )
            .expect("it composes");
        let rendered = String::from_utf8(message.formatted()).expect("text");

        assert!(rendered.contains("no-reply@example.com"), "{rendered}");
        assert!(rendered.contains("person@example.com"), "{rendered}");
        assert!(rendered.contains("ft_pr_abc"), "{rendered}");
        assert!(rendered.contains("works once"), "{rendered}");
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
