use std::{
    env, fs,
    path::{Path, PathBuf},
};

use miette::Diagnostic;
use serde::Deserialize;
use thiserror::Error;

const DATA_HOME_ENVIRONMENT_VARIABLE: &str = "KAT_DATA_HOME";

#[derive(Default, Deserialize)]
struct Configuration {
    #[serde(default, deserialize_with = "deserialize_optional_nonnull")]
    kat_data_home: Option<String>,
}

fn deserialize_optional_nonnull<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    String::deserialize(deserializer).map(Some)
}

pub(super) fn data_home() -> Result<PathBuf, ConfigurationError> {
    if let Some(data_home) = environment_data_home()? {
        return Ok(data_home);
    }
    if let Some(data_home) = configured_data_home()? {
        return Ok(data_home);
    }
    default_data_home()
}

pub(super) fn default_data_home() -> Result<PathBuf, ConfigurationError> {
    directories::ProjectDirs::from("", "", "KAT")
        .map(|directories| directories.data_dir().to_path_buf())
        .ok_or(ConfigurationError::PlatformDataHomeUnavailable)
}

fn configured_data_home() -> Result<Option<PathBuf>, ConfigurationError> {
    let platform_data_home = default_data_home()?;
    let configuration = read_configuration(&platform_data_home)?;
    configuration
        .kat_data_home
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|path| canonical_directory(path, "kat_data_home in KAT Configuration"))
        .transpose()
}

fn environment_data_home() -> Result<Option<PathBuf>, ConfigurationError> {
    env::var_os(DATA_HOME_ENVIRONMENT_VARIABLE)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|path| canonical_directory(path, DATA_HOME_ENVIRONMENT_VARIABLE))
        .transpose()
}

fn read_configuration(platform_data_home: &Path) -> Result<Configuration, ConfigurationError> {
    let path = platform_data_home.join("config.json");
    match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|source| ConfigurationError::InvalidConfiguration { path, source }),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(Configuration::default())
        }
        Err(source) => Err(ConfigurationError::ReadConfiguration { path, source }),
    }
}

fn canonical_directory(
    path: PathBuf,
    candidate: &'static str,
) -> Result<PathBuf, ConfigurationError> {
    if !path.is_absolute() {
        return Err(ConfigurationError::DataHomeNotAbsolute { candidate, path });
    }
    let canonical =
        dunce::canonicalize(&path).map_err(|error| ConfigurationError::CanonicalDirectory {
            candidate,
            path,
            error,
        })?;
    if !canonical.is_dir() {
        return Err(ConfigurationError::NotDirectory {
            candidate,
            path: canonical,
        });
    }
    Ok(canonical)
}

#[derive(Debug, Error, Diagnostic)]
pub(super) enum ConfigurationError {
    #[error("KAT Data Home is unavailable on this platform")]
    PlatformDataHomeUnavailable,
    #[error("failed to read KAT Configuration {path}")]
    ReadConfiguration {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("KAT Configuration is invalid: {path}")]
    InvalidConfiguration {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("{candidate} must be an absolute path, got {path}")]
    DataHomeNotAbsolute {
        candidate: &'static str,
        path: PathBuf,
    },
    #[error("failed to resolve {candidate} directory {path}")]
    CanonicalDirectory {
        candidate: &'static str,
        path: PathBuf,
        #[source]
        error: std::io::Error,
    },
    #[error("{candidate} is not a directory: {path}")]
    NotDirectory {
        candidate: &'static str,
        path: PathBuf,
    },
}
