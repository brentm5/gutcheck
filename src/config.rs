//! Loads settings from the config file. See `docs/configuration.md`.

use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::Url;
use serde::{Deserialize, Deserializer};

use crate::error::Error;

/// Settings after applying defaults, the config file and flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub model: String,
    pub retries: u32,
    pub timeout: Duration,
    pub base_url: Url,
}

/// Values given as command-line flags, which win over the config file.
#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub model: Option<String>,
    pub timeout: Option<Duration>,
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ConfigFile {
    model: String,
    retries: u32,
    #[serde(deserialize_with = "deserialize_duration")]
    timeout: Duration,
    base_url: String,
}

impl Default for ConfigFile {
    fn default() -> Self {
        Self {
            model: "jev-latest".to_owned(),
            retries: 2,
            timeout: Duration::from_secs(30),
            base_url: "https://api.typesafe.ai".to_owned(),
        }
    }
}

impl Settings {
    /// Loads settings from `explicit_path` (`--config`), or else the default
    /// location, then applies `overrides`.
    ///
    /// An explicit path must exist. The default one is optional.
    pub fn load(explicit_path: Option<&Path>, overrides: Overrides) -> Result<Self, Error> {
        let contents = match explicit_path {
            Some(path) => match fs::read_to_string(path) {
                Ok(contents) => Some(contents),
                Err(e) if e.kind() == ErrorKind::NotFound => {
                    return Err(Error::Usage(format!(
                        "config file not found: {}",
                        path.display()
                    )));
                }
                Err(e) => return Err(read_error(path, &e)),
            },
            None => match default_config_path() {
                Some(path) => read_optional(&path)?,
                None => None,
            },
        };

        let file = match contents {
            Some(contents) => toml::from_str(&contents)
                .map_err(|e| Error::Usage(format!("invalid config file: {}", e.message())))?,
            None => ConfigFile::default(),
        };

        Ok(Self {
            model: overrides.model.unwrap_or(file.model),
            retries: file.retries,
            timeout: overrides.timeout.unwrap_or(file.timeout),
            base_url: parse_base_url(&file.base_url)?,
        })
    }
}

/// Reads the default config file. It's missing if the file doesn't exist, or if
/// a parent of the path isn't a directory.
fn read_optional(path: &Path) -> Result<Option<String>, Error> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => Ok(None),
        Err(e) => Err(read_error(path, &e)),
    }
}

fn read_error(path: &Path, error: &std::io::Error) -> Error {
    Error::Usage(format!(
        "could not read config file {}: {error}",
        path.display()
    ))
}

fn default_config_path() -> Option<PathBuf> {
    config_path_from(
        env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        env::var_os("HOME").map(PathBuf::from),
    )
}

/// `$XDG_CONFIG_HOME/gutcheck/config.toml`, falling back to `~/.config`.
/// Empty or relative values count as unset, so an untrusted current directory
/// can't supply the config file (and with it the `base_url` that gets the key).
fn config_path_from(xdg_config_home: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
    let config_dir = xdg_config_home
        .filter(|dir| dir.is_absolute())
        .or_else(|| {
            home.filter(|dir| dir.is_absolute())
                .map(|home| home.join(".config"))
        })?;
    Some(config_dir.join("gutcheck").join("config.toml"))
}

/// Checks `base_url`. Error messages don't repeat the value, since it could
/// contain a password.
fn parse_base_url(value: &str) -> Result<Url, Error> {
    let invalid = |problem: &str| Error::Usage(format!("invalid config file: base_url: {problem}"));

    let url = Url::parse(value).map_err(|_| invalid("must be an absolute http or https URL"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(invalid("must be an absolute http or https URL"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid("must not contain a username or password"));
    }
    // API paths are appended to the URL, so these would swallow them.
    if url.query().is_some() || url.fragment().is_some() {
        return Err(invalid("must not contain a query or fragment"));
    }
    Ok(url)
}

/// Parses a duration such as `500ms`, `10s` or `1m`.
pub fn parse_duration(value: &str) -> Result<Duration, String> {
    let invalid = || format!("invalid duration {value:?}: expected a form like 500ms, 10s or 1m");

    let unit_start = value
        .find(|c: char| !c.is_ascii_digit())
        .ok_or_else(invalid)?;
    let (amount, unit) = value.split_at(unit_start);
    let amount: u64 = amount.parse().map_err(|_| invalid())?;

    match unit {
        "ms" => Ok(Duration::from_millis(amount)),
        "s" => Ok(Duration::from_secs(amount)),
        "m" => amount
            .checked_mul(60)
            .map(Duration::from_secs)
            .ok_or_else(invalid),
        _ => Err(invalid()),
    }
}

fn deserialize_duration<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
    let value = String::deserialize(deserializer)?;
    parse_duration(&value).map_err(serde::de::Error::custom)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use super::*;

    fn config_file(contents: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(contents.as_bytes()).unwrap();
        file
    }

    fn load(contents: &str) -> Result<Settings, Error> {
        let file = config_file(contents);
        Settings::load(Some(file.path()), Overrides::default())
    }

    #[test]
    fn empty_file_uses_defaults() {
        let settings = load("").unwrap();
        assert_eq!(settings.model, "jev-latest");
        assert_eq!(settings.retries, 2);
        assert_eq!(settings.timeout, Duration::from_secs(30));
        assert_eq!(settings.base_url.as_str(), "https://api.typesafe.ai/");
    }

    #[test]
    fn file_overrides_defaults() {
        let settings = load(
            r#"
            model = "jev-1"
            retries = 0
            timeout = "500ms"
            base_url = "http://localhost:8080/typesafe"
            "#,
        )
        .unwrap();
        assert_eq!(settings.model, "jev-1");
        assert_eq!(settings.retries, 0);
        assert_eq!(settings.timeout, Duration::from_millis(500));
        assert_eq!(settings.base_url.as_str(), "http://localhost:8080/typesafe");
    }

    #[test]
    fn flags_override_file() {
        let file = config_file("model = \"jev-1\"\ntimeout = \"5s\"");
        let settings = Settings::load(
            Some(file.path()),
            Overrides {
                model: Some("jev-2".to_owned()),
                timeout: Some(Duration::from_secs(1)),
            },
        )
        .unwrap();
        assert_eq!(settings.model, "jev-2");
        assert_eq!(settings.timeout, Duration::from_secs(1));
    }

    #[test]
    fn explicit_path_must_exist() {
        let error =
            Settings::load(Some(Path::new("/nonexistent.toml")), Overrides::default()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "config file not found: /nonexistent.toml"
        );
    }

    #[test]
    fn rejects_bad_base_urls() {
        for base_url in ["api.typesafe.ai", "/v1", "ftp://example.com"] {
            let error = load(&format!("base_url = {base_url:?}")).unwrap_err();
            assert!(error.to_string().contains("base_url"), "{error}");
        }
    }

    #[test]
    fn rejects_base_urls_with_a_query_or_fragment() {
        for base_url in [
            "http://localhost/?t=1",
            "http://localhost/?",
            "http://localhost/#a",
        ] {
            let message = load(&format!("base_url = {base_url:?}"))
                .unwrap_err()
                .to_string();
            assert!(message.contains("query or fragment"), "{message}");
        }
    }

    #[test]
    fn rejects_base_urls_with_credentials_without_repeating_them() {
        for base_url in [
            "http://user@localhost",
            "http://user:hunter2@localhost",
            "http://:hunter2@localhost",
        ] {
            let message = load(&format!("base_url = {base_url:?}"))
                .unwrap_err()
                .to_string();
            assert!(message.contains("username or password"), "{message}");
            assert!(!message.contains("hunter2"), "{message}");
        }
    }

    #[test]
    fn rejects_unknown_settings() {
        let message = load("base-url = \"http://localhost\"")
            .unwrap_err()
            .to_string();
        assert!(message.contains("base-url"), "{message}");
        assert!(!message.contains('\n'), "{message}");
    }

    #[test]
    fn default_config_is_missing_when_a_parent_is_not_a_directory() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path().join("gutcheck").join("config.toml");
        assert_eq!(read_optional(&path).unwrap(), None);
    }

    #[test]
    fn default_config_is_missing_when_the_file_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            read_optional(&dir.path().join("config.toml")).unwrap(),
            None
        );
    }

    #[test]
    fn default_config_other_read_errors_are_reported() {
        // Reading a directory as a file fails with something other than "missing".
        let dir = tempfile::tempdir().unwrap();
        assert!(read_optional(dir.path()).is_err());
    }

    #[test]
    fn rejects_negative_retries() {
        assert!(load("retries = -1").is_err());
    }

    #[test]
    fn config_errors_are_one_line() {
        for contents in ["not toml", "timeout = \"soon\"", "retries = -1"] {
            let message = load(contents).unwrap_err().to_string();
            assert!(!message.contains('\n'), "{message}");
        }
    }

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration("500ms"), Ok(Duration::from_millis(500)));
        assert_eq!(parse_duration("10s"), Ok(Duration::from_secs(10)));
        assert_eq!(parse_duration("1m"), Ok(Duration::from_secs(60)));
    }

    #[test]
    fn rejects_bad_durations() {
        for value in [
            "",
            "10",
            "s",
            "1h",
            "-1s",
            "1.5s",
            "1 s",
            "99999999999999999999s",
        ] {
            assert!(parse_duration(value).is_err(), "{value:?}");
        }
        assert!(parse_duration(&format!("{}m", u64::MAX)).is_err());
    }

    #[test]
    fn xdg_config_home_wins_over_home() {
        assert_eq!(
            config_path_from(Some("/xdg".into()), Some("/home/me".into())),
            Some(PathBuf::from("/xdg/gutcheck/config.toml"))
        );
    }

    #[test]
    fn falls_back_to_dot_config() {
        let expected = Some(PathBuf::from("/home/me/.config/gutcheck/config.toml"));
        assert_eq!(config_path_from(None, Some("/home/me".into())), expected);
        assert_eq!(
            config_path_from(Some("".into()), Some("/home/me".into())),
            expected
        );
    }

    #[test]
    fn no_config_path_without_home() {
        assert_eq!(config_path_from(None, None), None);
    }

    #[test]
    fn relative_xdg_config_home_falls_back_to_home() {
        assert_eq!(
            config_path_from(Some("relative".into()), Some("/home/me".into())),
            Some(PathBuf::from("/home/me/.config/gutcheck/config.toml"))
        );
    }

    #[test]
    fn empty_or_relative_home_gives_no_config_path() {
        assert_eq!(config_path_from(None, Some("".into())), None);
        assert_eq!(config_path_from(None, Some("relative".into())), None);
        assert_eq!(config_path_from(Some("".into()), Some("".into())), None);
    }
}
