use ferristrace_domain::{Email, Mailer, ResetError, ResetToken};
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
            from: "Ferristrace <no-reply@example.com>".to_owned(),
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
