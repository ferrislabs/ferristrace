use std::net::SocketAddr;

use figment::Figment;
use figment::providers::{Env, Serialized};
use serde::{Deserialize, Serialize};

const PREFIX: &str = "FERRISTRACE_MCP_";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    pub listen: SocketAddr,
    pub api_url: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: SocketAddr::from(([0, 0, 0, 0], 8081)),
            api_url: "http://localhost:3333".to_owned(),
        }
    }
}

pub fn figment() -> Figment {
    Figment::from(Serialized::defaults(Config::default())).merge(Env::prefixed(PREFIX))
}

pub fn load() -> Result<Config, Box<figment::Error>> {
    figment().extract().map_err(Box::new)
}

#[cfg(test)]
#[allow(clippy::result_large_err)]
mod tests {
    use super::*;

    #[test]
    fn defaults_listen_on_8081_and_point_at_the_local_server() {
        figment::Jail::expect_with(|_| {
            let config: Config = figment().extract()?;
            assert_eq!(config, Config::default());
            assert_eq!(config.listen.port(), 8081);
            Ok(())
        });
    }

    #[test]
    fn the_environment_overrides_both_settings() {
        figment::Jail::expect_with(|jail| {
            jail.set_env("FERRISTRACE_MCP_API_URL", "http://ferristrace:3333");
            jail.set_env("FERRISTRACE_MCP_LISTEN", "127.0.0.1:9000");
            let config: Config = figment().extract()?;
            assert_eq!(config.api_url, "http://ferristrace:3333");
            assert_eq!(config.listen, "127.0.0.1:9000".parse().unwrap());
            Ok(())
        });
    }
}
