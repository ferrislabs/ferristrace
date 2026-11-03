use std::io::ErrorKind;
use std::path::PathBuf;

use figment::value::{Dict, Map, Value};
use figment::{Error, Metadata, Profile, Provider};

pub const PATH: &str = "FERRISTRACE_ENV_FILE";

#[derive(Debug)]
pub struct EnvFile {
    path: Option<PathBuf>,
    prefix: String,
}

impl EnvFile {
    #[must_use]
    pub fn prefixed(prefix: &str) -> Self {
        Self {
            path: std::env::var_os(PATH).map(PathBuf::from),
            prefix: prefix.to_ascii_uppercase(),
        }
    }

    #[must_use]
    pub fn at(path: impl Into<PathBuf>, prefix: &str) -> Self {
        Self {
            path: Some(path.into()),
            prefix: prefix.to_ascii_uppercase(),
        }
    }

    fn read(&self) -> std::io::Result<Option<String>> {
        let Some(path) = &self.path else {
            return Ok(None);
        };
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn setting<'a>(&self, line: &'a str) -> Option<(String, &'a str)> {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let (key, value) = line.split_once('=')?;
        let key = key.trim();
        let key = key.strip_prefix("export ").unwrap_or(key).trim();
        let rest = key
            .get(..self.prefix.len())
            .filter(|head| head.eq_ignore_ascii_case(&self.prefix))
            .map(|_| &key[self.prefix.len()..])?;
        if rest.is_empty() {
            return None;
        }
        Some((
            rest.to_ascii_lowercase().replace("__", "."),
            unquoted(value.trim()),
        ))
    }
}

fn unquoted(value: &str) -> &str {
    ['"', '\'']
        .into_iter()
        .find_map(|quote| value.strip_prefix(quote)?.strip_suffix(quote))
        .unwrap_or(value)
}

fn parsed(value: &str) -> Value {
    match value.parse::<Value>() {
        Ok(parsed) => parsed,
        Err(never) => match never {},
    }
}

fn merged(mut into: Dict, from: Dict) -> Dict {
    for (key, value) in from {
        let next = match (into.remove(&key), value) {
            (Some(Value::Dict(_, old)), Value::Dict(tag, new)) => {
                Value::Dict(tag, merged(old, new))
            }
            (_, value) => value,
        };
        into.insert(key, next);
    }
    into
}

impl Provider for EnvFile {
    fn metadata(&self) -> Metadata {
        let name = self.path.as_ref().map_or_else(
            || "settings file".to_owned(),
            |path| format!("settings file {}", path.display()),
        );
        Metadata::named(name)
    }

    fn data(&self) -> Result<Map<Profile, Dict>, Error> {
        let text = self
            .read()
            .map_err(|error| Error::from(format!("{}: {error}", self.metadata().name)))?;
        let Some(text) = text else {
            return Ok(Map::new());
        };
        let dict = text
            .lines()
            .filter_map(|line| self.setting(line))
            .filter_map(|(key, value)| figment::util::nest(&key, parsed(value)).into_dict())
            .fold(Dict::new(), merged);
        Ok(Profile::Default.collect(dict))
    }
}

#[cfg(test)]
#[allow(clippy::result_large_err)]
mod tests {
    use figment::Figment;
    use figment::providers::{Env, Serialized};
    use serde::{Deserialize, Serialize};

    use super::EnvFile;

    #[derive(Debug, Default, Deserialize, Serialize, PartialEq)]
    struct Client {
        id: Option<String>,
        secret: Option<String>,
    }

    #[derive(Debug, Default, Deserialize, Serialize, PartialEq)]
    struct Settings {
        issuer: Option<String>,
        port: Option<u16>,
        client: Client,
    }

    fn settings(file: &str) -> Figment {
        Figment::from(Serialized::defaults(Settings::default()))
            .merge(EnvFile::at(file, "APP_"))
            .merge(Env::prefixed("APP_").split("__"))
    }

    #[test]
    fn a_file_written_after_the_container_was_made_is_read_at_start() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                "app.env",
                "# written by the init job\nAPP_ISSUER=http://ferriskey:3333/realms/x\nAPP_CLIENT__ID=ferristrace\nAPP_CLIENT__SECRET=\"s3cret\"\nAPP_PORT=4318\nOTHER_KEY=ignored\n",
            )?;
            let read: Settings = settings("app.env").extract()?;
            assert_eq!(
                read.issuer.as_deref(),
                Some("http://ferriskey:3333/realms/x")
            );
            assert_eq!(read.client.id.as_deref(), Some("ferristrace"));
            assert_eq!(read.client.secret.as_deref(), Some("s3cret"));
            assert_eq!(read.port, Some(4318));
            Ok(())
        });
    }

    #[test]
    fn the_environment_wins_over_the_file() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                "app.env",
                "APP_ISSUER=from-file\nAPP_CLIENT__ID=from-file\n",
            )?;
            jail.set_env("APP_ISSUER", "from-env");
            let read: Settings = settings("app.env").extract()?;
            assert_eq!(read.issuer.as_deref(), Some("from-env"));
            assert_eq!(read.client.id.as_deref(), Some("from-file"));
            Ok(())
        });
    }

    #[test]
    fn a_missing_file_is_no_setting_at_all() {
        figment::Jail::expect_with(|_jail| {
            let read: Settings = settings("absent.env").extract()?;
            assert_eq!(read, Settings::default());
            Ok(())
        });
    }
}
