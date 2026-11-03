use std::net::SocketAddr;
use std::time::Duration;

use ferristrace_env_file::EnvFile;
use figment::Figment;
use figment::providers::{Env, Serialized};
use reqwest::Url;
use serde::{Deserialize, Serialize};

use crate::secret::Secret;

const PREFIX: &str = "FERRISTRACE_MCP_GATEWAY_";
const TOKEN_PATH: &str = "protocol/openid-connect/token";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    pub listen: SocketAddr,
    pub public_url: String,
    pub upstream: String,
    pub issuer: String,
    pub api_url: String,
    pub agent_audience: String,
    pub scopes_supported: Vec<String>,
    pub upstream_timeout_ms: u64,
    pub body_timeout_ms: u64,
    pub pdp: PdpConfig,
    pub exchange: ExchangeConfig,
    pub ingest_key: Option<Secret>,
    pub ingest_endpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PdpConfig {
    pub url: String,
    pub realm: String,
    pub client_id: String,
    pub client_secret: Secret,
    pub token_url: Option<String>,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExchangeConfig {
    pub token_url: Option<String>,
    pub client_id: String,
    pub client_secret: Secret,
    pub timeout_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: SocketAddr::from(([0, 0, 0, 0], 8080)),
            public_url: String::new(),
            upstream: "http://localhost:8081".to_owned(),
            issuer: String::new(),
            api_url: "http://localhost:3333".to_owned(),
            agent_audience: String::new(),
            scopes_supported: [
                "telemetry:read",
                "incidents:comment",
                "incidents:resolve",
                "oncall:acknowledge",
            ]
            .map(str::to_owned)
            .to_vec(),
            upstream_timeout_ms: 30_000,
            body_timeout_ms: 10_000,
            pdp: PdpConfig {
                url: String::new(),
                realm: String::new(),
                client_id: String::new(),
                client_secret: Secret::default(),
                token_url: None,
                timeout_ms: 1_500,
            },
            exchange: ExchangeConfig {
                token_url: None,
                client_id: String::new(),
                client_secret: Secret::default(),
                timeout_ms: 3_000,
            },
            ingest_key: None,
            ingest_endpoint: None,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("these settings are missing: {0}")]
    Missing(String),
    #[error("these settings are not usable: {0}")]
    Invalid(String),
}

pub fn figment() -> Figment {
    Figment::from(Serialized::defaults(Config::default()))
        .merge(EnvFile::prefixed(PREFIX))
        .merge(Env::prefixed(PREFIX).split("__"))
}

pub fn load() -> Result<Config, Box<figment::Error>> {
    figment().extract().map_err(Box::new)
}

fn usable(raw: &str) -> bool {
    Url::parse(raw).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && !url.cannot_be_a_base()
            && url.query().is_none()
            && url.fragment().is_none()
    })
}

impl Config {
    pub fn validate(&self) -> Result<(), ConfigError> {
        let mut missing = Vec::new();
        let mut invalid = Vec::new();
        let mut text = |name: &'static str, value: &str| {
            if value.trim().is_empty() {
                missing.push(name);
            }
        };
        text("public_url", &self.public_url);
        text("issuer", &self.issuer);
        text("agent_audience", &self.agent_audience);
        text("pdp.url", &self.pdp.url);
        text("pdp.realm", &self.pdp.realm);
        text("pdp.client_id", &self.pdp.client_id);
        text("pdp.client_secret", self.pdp.client_secret.expose());
        text("exchange.client_id", &self.exchange.client_id);
        text(
            "exchange.client_secret",
            self.exchange.client_secret.expose(),
        );
        if !missing.is_empty() {
            return Err(ConfigError::Missing(missing.join(", ")));
        }
        let mut url = |name: &'static str, value: &str| {
            if !usable(value) {
                invalid.push(name);
            }
        };
        url("public_url", &self.public_url);
        url("upstream", &self.upstream);
        url("issuer", &self.issuer);
        url("api_url", &self.api_url);
        url("pdp.url", &self.pdp.url);
        if let Some(token_url) = &self.pdp.token_url {
            url("pdp.token_url", token_url);
        }
        if let Some(token_url) = &self.exchange.token_url {
            url("exchange.token_url", token_url);
        }
        if self.pdp.realm.contains(['/', '?', '#']) {
            invalid.push("pdp.realm");
        }
        if self.pdp.timeout_ms == 0 {
            invalid.push("pdp.timeout_ms");
        }
        if self.exchange.timeout_ms == 0 {
            invalid.push("exchange.timeout_ms");
        }
        if self.upstream_timeout_ms == 0 {
            invalid.push("upstream_timeout_ms");
        }
        if self.body_timeout_ms == 0 {
            invalid.push("body_timeout_ms");
        }
        match (&self.ingest_key, &self.ingest_endpoint) {
            (Some(key), Some(endpoint)) if !key.is_empty() => {
                if !usable(endpoint) {
                    invalid.push("ingest_endpoint");
                }
            }
            (Some(key), None) if !key.is_empty() => {
                return Err(ConfigError::Missing("ingest_endpoint".to_owned()));
            }
            _ => {}
        }
        if invalid.is_empty() {
            Ok(())
        } else {
            Err(ConfigError::Invalid(invalid.join(", ")))
        }
    }

    pub fn exports_decisions(&self) -> bool {
        self.ingest_key.as_ref().is_some_and(|key| !key.is_empty())
            && self.ingest_endpoint.is_some()
    }

    pub fn public_base(&self) -> &str {
        self.public_url.trim_end_matches('/')
    }

    pub fn exchange_token_url(&self) -> String {
        self.exchange
            .token_url
            .clone()
            .unwrap_or_else(|| format!("{}/{TOKEN_PATH}", self.issuer.trim_end_matches('/')))
    }

    pub fn pdp_token_url(&self) -> String {
        self.pdp
            .token_url
            .clone()
            .unwrap_or_else(|| format!("{}/{TOKEN_PATH}", self.issuer.trim_end_matches('/')))
    }

    pub fn evaluation_url(&self) -> String {
        self.authzen_url("evaluation")
    }

    pub fn evaluations_url(&self) -> String {
        self.authzen_url("evaluations")
    }

    fn authzen_url(&self, tail: &str) -> String {
        format!(
            "{}/realms/{}/authzen/access/v1/{tail}",
            self.pdp.url.trim_end_matches('/'),
            self.pdp.realm
        )
    }

    pub fn pdp_timeout(&self) -> Duration {
        Duration::from_millis(self.pdp.timeout_ms)
    }

    pub fn exchange_timeout(&self) -> Duration {
        Duration::from_millis(self.exchange.timeout_ms)
    }

    pub fn body_timeout(&self) -> Duration {
        Duration::from_millis(self.body_timeout_ms)
    }

    pub fn upstream_timeout(&self) -> Duration {
        Duration::from_millis(self.upstream_timeout_ms)
    }
}

#[cfg(test)]
#[allow(clippy::result_large_err)]
mod tests {
    use super::*;

    fn complete() -> Config {
        let defaults = Config::default();
        Config {
            public_url: "https://mcp.example.com/".to_owned(),
            issuer: "https://auth.example.com/realms/ferris".to_owned(),
            agent_audience: "ferristrace".to_owned(),
            pdp: PdpConfig {
                url: "https://auth.example.com".to_owned(),
                realm: "ferris".to_owned(),
                client_id: "ferristrace-pep".to_owned(),
                client_secret: Secret::new("pdp-secret"),
                ..defaults.pdp.clone()
            },
            exchange: ExchangeConfig {
                client_id: "ferristrace-mcp-gateway".to_owned(),
                client_secret: Secret::new("exchange-secret"),
                ..defaults.exchange.clone()
            },
            ..defaults
        }
    }

    #[test]
    fn the_defaults_listen_on_8080_with_a_one_and_a_half_second_pdp() {
        figment::Jail::expect_with(|_| {
            let config: Config = figment().extract()?;
            assert_eq!(config, Config::default());
            assert_eq!(config.listen.port(), 8080);
            assert_eq!(config.pdp_timeout(), Duration::from_millis(1500));
            assert_eq!(config.scopes_supported.len(), 4);
            Ok(())
        });
    }

    #[test]
    fn the_environment_fills_flat_and_nested_settings() {
        figment::Jail::expect_with(|jail| {
            jail.set_env(
                "FERRISTRACE_MCP_GATEWAY_PUBLIC_URL",
                "https://mcp.example.com",
            );
            jail.set_env("FERRISTRACE_MCP_GATEWAY_UPSTREAM", "http://mcp:8080");
            jail.set_env("FERRISTRACE_MCP_GATEWAY_PDP__CLIENT_SECRET", "s3cret");
            jail.set_env("FERRISTRACE_MCP_GATEWAY_PDP__TIMEOUT_MS", "900");
            jail.set_env("FERRISTRACE_MCP_GATEWAY_EXCHANGE__CLIENT_ID", "gateway");
            jail.set_env("FERRISTRACE_MCP_GATEWAY_INGEST_KEY", "ingest");
            let config: Config = figment().extract()?;
            assert_eq!(config.public_url, "https://mcp.example.com");
            assert_eq!(config.upstream, "http://mcp:8080");
            assert_eq!(config.pdp.client_secret.expose(), "s3cret");
            assert_eq!(config.pdp.timeout_ms, 900);
            assert_eq!(config.exchange.client_id, "gateway");
            assert_eq!(config.pdp.url, "");
            assert!(config.ingest_key.is_some());
            Ok(())
        });
    }

    #[test]
    fn a_complete_configuration_is_valid_and_derives_its_urls() {
        let config = complete();
        assert_eq!(config.validate(), Ok(()));
        assert_eq!(config.public_base(), "https://mcp.example.com");
        assert_eq!(
            config.exchange_token_url(),
            "https://auth.example.com/realms/ferris/protocol/openid-connect/token"
        );
        assert_eq!(config.pdp_token_url(), config.exchange_token_url());
        assert_eq!(
            config.evaluation_url(),
            "https://auth.example.com/realms/ferris/authzen/access/v1/evaluation"
        );
        assert_eq!(
            config.evaluations_url(),
            "https://auth.example.com/realms/ferris/authzen/access/v1/evaluations"
        );
        assert!(!config.exports_decisions());
    }

    #[test]
    fn a_missing_setting_is_named_and_a_secret_is_not() {
        let mut config = complete();
        config.pdp.client_secret = Secret::default();
        config.issuer.clear();
        let error = config.validate().unwrap_err();
        assert_eq!(
            error,
            ConfigError::Missing("issuer, pdp.client_secret".to_owned())
        );
        assert!(!error.to_string().contains("pdp-secret"));
    }

    #[test]
    fn an_unusable_url_or_timeout_is_refused() {
        let mut config = complete();
        config.upstream = "not a url".to_owned();
        config.pdp.timeout_ms = 0;
        config.public_url = "ftp://mcp.example.com".to_owned();
        assert_eq!(
            config.validate(),
            Err(ConfigError::Invalid(
                "public_url, upstream, pdp.timeout_ms".to_owned()
            ))
        );
    }

    #[test]
    fn the_debug_form_hides_every_secret() {
        let mut config = complete();
        config.ingest_key = Some(Secret::new("ingest-secret"));
        let printed = format!("{config:?}");
        assert!(!printed.contains("pdp-secret"));
        assert!(!printed.contains("exchange-secret"));
        assert!(!printed.contains("ingest-secret"));
    }

    #[test]
    fn an_ingest_key_needs_an_endpoint() {
        let mut config = complete();
        config.ingest_key = Some(Secret::new("k"));
        assert_eq!(
            config.validate(),
            Err(ConfigError::Missing("ingest_endpoint".to_owned()))
        );
        config.ingest_endpoint = Some("http://ferristrace:4318/v1/logs".to_owned());
        assert_eq!(config.validate(), Ok(()));
        assert!(config.exports_decisions());
    }

    #[test]
    fn explicit_token_urls_win() {
        let mut config = complete();
        config.pdp.token_url = Some("https://t.example/pdp".to_owned());
        config.exchange.token_url = Some("https://t.example/x".to_owned());
        assert_eq!(config.pdp_token_url(), "https://t.example/pdp");
        assert_eq!(config.exchange_token_url(), "https://t.example/x");
    }
}
