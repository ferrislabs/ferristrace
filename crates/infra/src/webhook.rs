use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use ferristrace_domain::{
    Chat, Culprits, DeliveryError, Design, Dispatch, WebhookSecret, WebhookSender, WebhookUrl,
};
use hmac::{Hmac, KeyInit, Mac};
use reqwest::Url;
use reqwest::header::CONTENT_TYPE;
use reqwest::redirect::Policy;
use serde_json::{Map, Value, json};
use sha2::Sha256;

pub const SIGNATURE_HEADER: &str = "X-FerrisTrace-Signature";
pub const WEBHOOK_TIMEOUT: Duration = Duration::from_secs(10);

const METADATA_HOSTS: [&str; 3] = ["metadata.google.internal", "metadata", "instance-data"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookSettings {
    pub allow_private: bool,
    pub base_url: String,
    pub timeout: Duration,
}

impl Default for WebhookSettings {
    fn default() -> Self {
        Self {
            allow_private: false,
            base_url: String::new(),
            timeout: WEBHOOK_TIMEOUT,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HttpWebhooks {
    settings: WebhookSettings,
}

impl HttpWebhooks {
    pub fn new(settings: WebhookSettings) -> Self {
        Self { settings }
    }

    pub fn link_to(&self, dispatch: &Dispatch) -> String {
        format!(
            "{}/alerts?rule={}",
            self.settings.base_url.trim_end_matches('/'),
            dispatch.rule
        )
    }

    pub fn body_of(&self, dispatch: &Dispatch) -> Vec<u8> {
        let mut body = json!({
            "alert": {
                "id": dispatch.rule.to_string(),
                "name": &*dispatch.name,
                "severity": dispatch.severity.as_str(),
            },
            "from": dispatch.from.as_str(),
            "to": dispatch.to.as_str(),
            "value": dispatch.value,
            "labels": Value::Object(
                dispatch
                    .labels
                    .pairs()
                    .iter()
                    .map(|(key, value)| (key.to_string(), Value::String(value.to_string())))
                    .collect::<Map<_, _>>()
            ),
            "at_ms": dispatch.at_ms,
            "org": dispatch.org.to_string(),
            "url_to_alert": self.link_to(dispatch),
        });
        if let (Some(context), Some(fields)) = (&dispatch.context, body.as_object_mut()) {
            fields.insert("context".to_owned(), context_of(context));
        }
        body.to_string().into_bytes()
    }

    fn refuses(&self, address: IpAddr) -> bool {
        !self.settings.allow_private && private(address)
    }
}

fn context_of(context: &Culprits) -> Value {
    json!({
        "pods": context
            .pods()
            .iter()
            .map(|pod| json!({
                "name": &*pod.name,
                "node": pod.node.as_deref(),
                "count": pod.count,
            }))
            .collect::<Vec<_>>(),
        "others": context.others(),
        "nodes": context.nodes(),
        "versions": context.versions(),
        "labels": Value::Object(
            context
                .labels()
                .pairs()
                .iter()
                .map(|(key, value)| (key.to_string(), Value::String(value.to_string())))
                .collect::<Map<_, _>>()
        ),
    })
}

pub fn signature(secret: &WebhookSecret, body: &[u8]) -> String {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(secret.exposed().as_bytes())
        .expect("hmac takes a key of any length");
    mac.update(body);
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

pub fn private(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => private_v4(v4),
        IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .map_or_else(|| private_v6(v6), private_v4),
    }
}

fn private_v4(address: Ipv4Addr) -> bool {
    let [first, second, ..] = address.octets();
    address.is_private()
        || address.is_loopback()
        || address.is_link_local()
        || address.is_unspecified()
        || address.is_broadcast()
        || address.is_multicast()
        || first == 0
        || (first == 100 && (64..128).contains(&second))
        || (first == 198 && (18..20).contains(&second))
        || first >= 240
}

fn private_v6(address: Ipv6Addr) -> bool {
    let first = address.segments()[0];
    address.is_loopback()
        || address.is_unspecified()
        || address.is_multicast()
        || (first & 0xfe00) == 0xfc00
        || (first & 0xffc0) == 0xfe80
}

fn named_privately(host: &str) -> bool {
    let host = host.trim_end_matches('.');
    host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".internal")
        || METADATA_HOSTS.contains(&host)
}

fn literal(host: &str) -> Option<IpAddr> {
    host.trim_start_matches('[')
        .trim_end_matches(']')
        .parse()
        .ok()
}

impl HttpWebhooks {
    async fn post(
        &self,
        url: &WebhookUrl,
        body: Vec<u8>,
        signed_with: Option<&WebhookSecret>,
    ) -> Result<(), DeliveryError> {
        if !self.admits(url) {
            return Err(DeliveryError::new(
                "this deployment does not send webhooks to private addresses",
            ));
        }
        let parsed = Url::parse(url.as_str())
            .map_err(|_| DeliveryError::new("the webhook url does not parse"))?;
        let host = parsed
            .host_str()
            .ok_or_else(|| DeliveryError::new("the webhook url names no host"))?
            .to_owned();
        let port = parsed
            .port_or_known_default()
            .ok_or_else(|| DeliveryError::new("the webhook url names no port"))?;
        let addresses: Vec<SocketAddr> = tokio::net::lookup_host((url.host(), port))
            .await
            .map_err(|_| DeliveryError::new("the webhook host does not resolve"))?
            .collect();
        if addresses.is_empty() {
            return Err(DeliveryError::new("the webhook host does not resolve"));
        }
        if addresses.iter().any(|address| self.refuses(address.ip())) {
            return Err(DeliveryError::new(
                "the webhook host resolves to a private address",
            ));
        }
        let client = reqwest::Client::builder()
            .timeout(self.settings.timeout)
            .redirect(Policy::none())
            .resolve_to_addrs(&host, &addresses)
            .build()
            .map_err(|_| DeliveryError::new("the webhook client could not be prepared"))?;
        let mut request = client.post(parsed).header(CONTENT_TYPE, "application/json");
        if let Some(secret) = signed_with {
            request = request.header(SIGNATURE_HEADER, signature(secret, &body));
        }
        let answer = request.body(body).send().await.map_err(|error| {
            if error.is_timeout() {
                DeliveryError::new("the webhook did not answer in time")
            } else {
                DeliveryError::new("the webhook could not be reached")
            }
        })?;
        if answer.status().is_success() {
            Ok(())
        } else {
            Err(DeliveryError(
                format!("the webhook answered {}", answer.status().as_u16()).into(),
            ))
        }
    }
}

impl WebhookSender for HttpWebhooks {
    fn admits(&self, url: &WebhookUrl) -> bool {
        if self.settings.allow_private {
            return true;
        }
        match literal(url.host()) {
            Some(address) => !private(address),
            None => !named_privately(url.host()),
        }
    }

    async fn send(
        &self,
        url: &WebhookUrl,
        secret: &WebhookSecret,
        dispatch: &Dispatch,
    ) -> Result<(), DeliveryError> {
        self.post(url, self.body_of(dispatch), Some(secret)).await
    }

    async fn send_chat(
        &self,
        chat: Chat,
        url: &WebhookUrl,
        design: &Design,
        dispatch: &Dispatch,
    ) -> Result<(), DeliveryError> {
        let message = self.render_chat(chat, design, dispatch);
        self.post(url, message.into_bytes(), None).await
    }

    fn render_chat(&self, chat: Chat, design: &Design, dispatch: &Dispatch) -> String {
        let link = self.link_to(dispatch);
        match chat {
            Chat::Slack => crate::chat::slack(dispatch, &link),
            Chat::Discord => crate::chat::discord(design, dispatch, &link),
        }
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use ferristrace_domain::{WebhookSecret, WebhookSender, WebhookUrl};

    use super::{HttpWebhooks, WebhookSettings, private, signature};

    #[test]
    fn the_context_joins_the_payload_only_when_something_is_known() {
        use std::collections::BTreeMap;

        use ferristrace_domain::{
            AlertSeverity, AlertStatus, CulpritPod, Culprits, Dispatch, Labels, RuleId, TenantId,
        };
        use uuid::Uuid;

        let mut dispatch = Dispatch {
            rule: RuleId::new(Uuid::from_u128(7)),
            name: "Queue".into(),
            severity: AlertSeverity::Warning,
            org: TenantId::new(Uuid::from_u128(1)),
            from: AlertStatus::Pending,
            to: AlertStatus::Firing,
            value: Some(3.0),
            labels: Labels::none(),
            at_ms: 5,
            context: Some(Culprits::new(
                vec![CulpritPod {
                    name: "mail-1".into(),
                    node: Some("node-a".into()),
                    count: 4,
                }],
                1,
                vec!["node-a".into()],
                vec!["2.0.1".into()],
                &Labels::new(BTreeMap::from([("queue".into(), "mail".into())])),
            )),
        };
        let hooks = HttpWebhooks::new(WebhookSettings::default());

        let with: serde_json::Value =
            serde_json::from_slice(&hooks.body_of(&dispatch)).expect("json");
        dispatch.context = None;
        let without: serde_json::Value =
            serde_json::from_slice(&hooks.body_of(&dispatch)).expect("json");

        assert_eq!(
            with["context"],
            serde_json::json!({
                "pods": [{"name": "mail-1", "node": "node-a", "count": 4}],
                "others": 0,
                "nodes": ["node-a"],
                "versions": ["2.0.1"],
                "labels": {"queue": "mail"},
            })
        );
        assert!(without.get("context").is_none(), "{without}");
        let mut stripped = with.clone();
        stripped
            .as_object_mut()
            .expect("an object")
            .remove("context");
        assert_eq!(stripped, without, "every other field is unchanged");
    }

    fn url(text: &str) -> WebhookUrl {
        WebhookUrl::try_from(text).expect("a url")
    }

    #[test]
    fn private_loopback_link_local_and_metadata_addresses_are_private() {
        for address in [
            "10.0.0.1",
            "172.16.4.4",
            "192.168.1.1",
            "127.0.0.1",
            "169.254.169.254",
            "0.0.0.0",
            "100.64.0.1",
            "::1",
            "fe80::1",
            "fd00:ec2::254",
            "::ffff:127.0.0.1",
        ] {
            let parsed: IpAddr = address.parse().expect("an address");
            assert!(private(parsed), "{address}");
        }
        for address in ["93.184.216.34", "2606:4700::1111", "8.8.8.8"] {
            let parsed: IpAddr = address.parse().expect("an address");
            assert!(!private(parsed), "{address}");
        }
    }

    #[test]
    fn a_private_target_is_refused_unless_the_deployment_allows_it() {
        let guarded = HttpWebhooks::new(WebhookSettings::default());
        let open = HttpWebhooks::new(WebhookSettings {
            allow_private: true,
            ..WebhookSettings::default()
        });

        for target in [
            "http://127.0.0.1:9000/in",
            "http://[::1]/in",
            "http://169.254.169.254/latest/meta-data",
            "http://localhost:8080/",
            "http://metadata.google.internal/",
            "https://10.1.2.3/hook",
        ] {
            assert!(!guarded.admits(&url(target)), "{target}");
            assert!(open.admits(&url(target)), "{target}");
        }
        assert!(guarded.admits(&url("https://hooks.example.com/in")));
    }

    #[test]
    fn the_signature_is_the_hex_hmac_sha256_of_the_body_under_the_secret() {
        let secret = WebhookSecret::from_plaintext("key");

        assert_eq!(
            signature(&secret, b"The quick brown fox jumps over the lazy dog"),
            "sha256=f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
        );
    }
}
