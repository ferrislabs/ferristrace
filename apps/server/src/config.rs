use std::net::SocketAddr;

use ferristrace_clickhouse::SpanStoreSettings;
use ferristrace_domain::DecompressedBytes;
use ferristrace_domain::telemetry::Allowance;
use ferristrace_infra::PhotoStoreSettings;
use figment::Figment;
use figment::providers::{Env, Format, Serialized, Toml};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const FILE: &str = "ferristrace.toml";
const PREFIX: &str = "FERRISTRACE_";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub grpc_address: SocketAddr,
    pub http_address: SocketAddr,
    #[serde(default)]
    pub database_url: Option<String>,
    #[serde(default)]
    pub redis_url: Option<String>,
    #[serde(default)]
    pub spans: Option<SpansConfig>,
    #[serde(default)]
    pub web_root: Option<String>,
    #[serde(default)]
    pub tenants: Vec<TenantKey>,
    #[serde(default)]
    pub oidc: Option<OidcConfig>,
    #[serde(default)]
    pub mail: Option<MailConfig>,
    #[serde(default)]
    pub objects: Option<ObjectsConfig>,
    #[serde(default)]
    pub secrets: Option<SecretsConfig>,
    #[serde(default)]
    pub alerts: AlertsConfig,
    #[serde(default = "yes")]
    pub local_password: bool,
    #[serde(default = "yes")]
    pub cookie_secure: bool,
    #[serde(default = "landing")]
    pub after_sign_in: String,
}

fn yes() -> bool {
    true
}

fn landing() -> String {
    "/".to_owned()
}

fn a_month() -> u64 {
    30
}

pub(crate) fn default_retention_days(spans: Option<&SpansConfig>) -> u64 {
    spans.map_or_else(a_month, |spans| spans.retention_days)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpansConfig {
    pub url: String,
    #[serde(default)]
    pub database: Option<String>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default = "a_month")]
    pub retention_days: u64,
}

impl From<SpansConfig> for SpanStoreSettings {
    fn from(spans: SpansConfig) -> Self {
        Self {
            url: spans.url,
            database: spans.database,
            user: spans.user,
            password: spans.password,
            retention_days: spans.retention_days,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailConfig {
    pub relay: String,
    pub username: String,
    pub password: String,
    pub from: String,
    pub base_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectsConfig {
    pub endpoint: String,
    pub bucket: String,
    #[serde(default)]
    pub region: Option<String>,
    pub access_key_id: String,
    pub secret_access_key: String,
}

impl From<ObjectsConfig> for PhotoStoreSettings {
    fn from(objects: ObjectsConfig) -> Self {
        Self {
            endpoint: objects.endpoint,
            bucket: objects.bucket,
            region: objects.region,
            access_key_id: objects.access_key_id,
            secret_access_key: objects.secret_access_key,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretsConfig {
    pub at_rest_key: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertsConfig {
    #[serde(default)]
    pub allow_private_webhooks: bool,
    #[serde(default)]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OidcConfig {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantKey {
    pub id: Uuid,
    #[serde(default)]
    pub allowance: TenantAllowance,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "bytes", rename_all = "snake_case")]
pub enum TenantAllowance {
    #[default]
    Unlimited,
    Capped(u64),
    Overage(u64),
}

impl From<TenantAllowance> for Allowance {
    fn from(allowance: TenantAllowance) -> Self {
        match allowance {
            TenantAllowance::Unlimited => Self::Unlimited,
            TenantAllowance::Capped(bytes) => Self::Capped(DecompressedBytes::new(bytes)),
            TenantAllowance::Overage(bytes) => Self::Overage(DecompressedBytes::new(bytes)),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            grpc_address: SocketAddr::from(([0, 0, 0, 0], 4317)),
            http_address: SocketAddr::from(([0, 0, 0, 0], 4318)),
            database_url: None,
            redis_url: None,
            spans: None,
            web_root: None,
            tenants: Vec::new(),
            oidc: None,
            mail: None,
            objects: None,
            secrets: None,
            alerts: AlertsConfig::default(),
            local_password: true,
            cookie_secure: true,
            after_sign_in: landing(),
        }
    }
}

pub fn figment() -> Figment {
    Figment::from(Serialized::defaults(Config::default()))
        .merge(Toml::file(FILE))
        .merge(Env::prefixed(PREFIX).split("__"))
}

pub fn load() -> anyhow::Result<Config> {
    figment().extract().map_err(Into::into)
}

#[cfg(test)]
#[allow(clippy::result_large_err)]
mod tests {
    use super::{Config, FILE, TenantAllowance, figment};

    #[test]
    fn the_default_ports_are_the_otlp_ones() {
        figment::Jail::expect_with(|_jail| {
            let config: Config = figment().extract()?;

            assert_eq!(config.grpc_address.port(), 4317);
            assert_eq!(config.http_address.port(), 4318);
            assert!(config.tenants.is_empty());
            Ok(())
        });
    }

    #[test]
    fn a_file_declares_tenant_keys() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                FILE,
                r#"
                http_address = "127.0.0.1:9318"

                [[tenants]]
                id = "00000000-0000-0000-0000-000000000001"
                "#,
            )?;

            let config: Config = figment().extract()?;

            assert_eq!(config.http_address.port(), 9318);
            assert_eq!(config.grpc_address.port(), 4317);
            assert_eq!(config.tenants.len(), 1);
            Ok(())
        });
    }

    #[test]
    fn a_tenant_without_an_allowance_is_unlimited() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                FILE,
                r#"
                [[tenants]]
                id = "00000000-0000-0000-0000-000000000001"
                "#,
            )?;

            let config: Config = figment().extract()?;

            assert_eq!(config.tenants[0].allowance, TenantAllowance::Unlimited);
            Ok(())
        });
    }

    #[test]
    fn an_allowance_declares_its_kind_and_its_bytes() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                FILE,
                r#"
                [[tenants]]
                id = "00000000-0000-0000-0000-000000000002"
                allowance = { kind = "capped", bytes = 1048576 }

                [[tenants]]
                id = "00000000-0000-0000-0000-000000000003"
                allowance = { kind = "overage", bytes = 10485760 }
                "#,
            )?;

            let config: Config = figment().extract()?;

            assert_eq!(
                config.tenants[0].allowance,
                TenantAllowance::Capped(1_048_576)
            );
            assert_eq!(
                config.tenants[1].allowance,
                TenantAllowance::Overage(10_485_760)
            );
            Ok(())
        });
    }

    #[test]
    fn a_span_store_needs_only_its_url_and_keeps_a_month() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                FILE,
                r#"
                [spans]
                url = "http://clickhouse:8123"
                "#,
            )?;

            let config: Config = figment().extract()?;
            let spans = config.spans.expect("a span store");

            assert_eq!(spans.url, "http://clickhouse:8123");
            assert_eq!(spans.retention_days, 30);
            assert_eq!(spans.database, None);
            Ok(())
        });
    }

    #[test]
    fn a_retention_in_the_file_wins_over_the_default() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                FILE,
                r#"
                [spans]
                url = "http://clickhouse:8123"
                database = "ferristrace"
                retention_days = 7
                "#,
            )?;

            let config: Config = figment().extract()?;
            let spans = config.spans.expect("a span store");

            assert_eq!(spans.retention_days, 7);
            assert_eq!(spans.database.as_deref(), Some("ferristrace"));
            Ok(())
        });
    }

    #[test]
    fn a_photo_store_needs_its_endpoint_bucket_and_credentials() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                FILE,
                r#"
                [objects]
                endpoint = "https://abc123.r2.cloudflarestorage.com"
                bucket = "ferristrace-avatars"
                access_key_id = "AKIAEXAMPLE"
                secret_access_key = "a secret"
                "#,
            )?;

            let config: Config = figment().extract()?;
            let objects = config.objects.expect("an object store");

            assert_eq!(objects.bucket, "ferristrace-avatars");
            assert_eq!(
                objects.region, None,
                "r2 has one region and the store names it"
            );
            Ok(())
        });
    }

    #[test]
    fn a_deployment_keeps_no_photos_unless_a_store_is_declared() {
        figment::Jail::expect_with(|_jail| {
            let config: Config = figment().extract()?;

            assert!(config.objects.is_none());
            Ok(())
        });
    }

    #[test]
    fn a_deployment_keeps_no_second_factor_unless_a_key_at_rest_is_declared() {
        figment::Jail::expect_with(|_jail| {
            let config: Config = figment().extract()?;

            assert!(config.secrets.is_none());
            Ok(())
        });
    }

    #[test]
    fn a_key_at_rest_is_read_from_the_file() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                FILE,
                r#"
                [secrets]
                at_rest_key = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff"
                "#,
            )?;

            let config: Config = figment().extract()?;
            let secrets = config.secrets.expect("a key");

            assert_eq!(secrets.at_rest_key.len(), 64);
            Ok(())
        });
    }

    #[test]
    fn webhooks_stay_off_private_addresses_unless_the_deployment_says_otherwise() {
        figment::Jail::expect_with(|jail| {
            let quiet: Config = figment().extract()?;
            jail.set_env("FERRISTRACE_ALERTS__ALLOW_PRIVATE_WEBHOOKS", "true");
            jail.set_env("FERRISTRACE_ALERTS__BASE_URL", "https://app.example.com");
            let open: Config = figment().extract()?;

            assert!(!quiet.alerts.allow_private_webhooks);
            assert_eq!(quiet.alerts.base_url, None);
            assert!(open.alerts.allow_private_webhooks);
            assert_eq!(
                open.alerts.base_url.as_deref(),
                Some("https://app.example.com")
            );
            Ok(())
        });
    }

    #[test]
    fn the_alert_settings_are_read_from_the_file() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                FILE,
                "
                [alerts]
                allow_private_webhooks = true
                ",
            )?;

            let config: Config = figment().extract()?;

            assert!(config.alerts.allow_private_webhooks);
            Ok(())
        });
    }

    #[test]
    fn a_mail_relay_is_declared_whole_or_not_at_all() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                FILE,
                r#"
                [mail]
                relay = "mail.example.com"
                username = "postmaster"
                password = "secret"
                from = "FerrisTrace <no-reply@example.com>"
                base_url = "https://app.example.com"
                "#,
            )?;

            let config: Config = figment().extract()?;
            let mail = config.mail.expect("a relay");

            assert_eq!(mail.relay, "mail.example.com");
            assert_eq!(mail.base_url, "https://app.example.com");
            Ok(())
        });
    }
}
