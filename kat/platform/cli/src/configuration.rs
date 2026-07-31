use std::{
    env, fs,
    path::{Path, PathBuf},
};

use config::{Config, ConfigError, File, FileFormat, Value, ValueKind};
use miette::Diagnostic;
use serde::Deserialize;
use thiserror::Error;

const DATA_HOME_ENVIRONMENT_VARIABLE: &str = "KAT_DATA_HOME";
const CONFIGURATION_FILE_NAME: &str = "config.json";

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
    let platform_data_home = default_data_home()?;
    let configuration_path = platform_data_home.join(CONFIGURATION_FILE_NAME);
    let file_configuration = read_configuration(&configuration_path)?;
    let environment_data_home = environment_data_home()?;
    let selected_source = environment_data_home
        .as_ref()
        .map(|_| DATA_HOME_ENVIRONMENT_VARIABLE)
        .unwrap_or("kat_data_home in KAT Configuration");
    let configuration = Config::builder()
        .add_source(file_configuration)
        .set_override_option("kat_data_home", environment_data_home)
        .map_err(|source| ConfigurationError::InvalidConfiguration {
            path: configuration_path.clone(),
            source: Box::new(source),
        })?
        .build()
        .and_then(|configuration| configuration.try_deserialize::<Configuration>())
        .map_err(|source| ConfigurationError::InvalidConfiguration {
            path: configuration_path,
            source: Box::new(source),
        })?;

    let selected = configuration
        .kat_data_home
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|path| canonical_directory(path, selected_source))
        .transpose()?;
    Ok(selected.unwrap_or(platform_data_home))
}

pub(super) fn default_data_home() -> Result<PathBuf, ConfigurationError> {
    directories::ProjectDirs::from("", "", "KAT")
        .map(|directories| directories.data_dir().to_path_buf())
        .ok_or(ConfigurationError::PlatformDataHomeUnavailable)
}

fn environment_data_home() -> Result<Option<String>, ConfigurationError> {
    env::var_os(DATA_HOME_ENVIRONMENT_VARIABLE)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .into_string()
                .map_err(|_| ConfigurationError::InvalidEnvironmentVariable)
        })
        .transpose()
}

fn read_configuration(path: &Path) -> Result<Config, ConfigurationError> {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            let bytes = fs::read(path).map_err(|source| ConfigurationError::ReadConfiguration {
                path: path.to_path_buf(),
                source,
            })?;
            let contents = std::str::from_utf8(&bytes).map_err(|source| {
                ConfigurationError::InvalidConfigurationEncoding {
                    path: path.to_path_buf(),
                    source,
                }
            })?;
            let configuration = Config::builder()
                .add_source(File::from_str(contents, FileFormat::Json))
                .build()
                .map_err(|source| ConfigurationError::InvalidConfiguration {
                    path: path.to_path_buf(),
                    source: Box::new(source),
                })?;
            validate_configuration_value_types(&configuration, path)?;
            Ok(configuration)
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(source) => Err(ConfigurationError::ReadConfiguration {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn validate_configuration_value_types(
    configuration: &Config,
    path: &Path,
) -> Result<(), ConfigurationError> {
    match configuration.get::<Value>("kat_data_home") {
        Ok(value) if matches!(value.kind, ValueKind::String(_)) => Ok(()),
        Ok(_) => Err(ConfigurationError::InvalidConfigurationValueType {
            path: path.to_path_buf(),
        }),
        Err(ConfigError::NotFound(_)) => Ok(()),
        Err(source) => Err(ConfigurationError::InvalidConfiguration {
            path: path.to_path_buf(),
            source: Box::new(source),
        }),
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
        source: Box<config::ConfigError>,
    },
    #[error("KAT Configuration is invalid: {path}; kat_data_home must be a string")]
    InvalidConfigurationValueType { path: PathBuf },
    #[error("KAT Configuration is invalid: {path}; file must contain valid UTF-8")]
    InvalidConfigurationEncoding {
        path: PathBuf,
        #[source]
        source: std::str::Utf8Error,
    },
    #[error("{DATA_HOME_ENVIRONMENT_VARIABLE} must contain valid Unicode")]
    InvalidEnvironmentVariable,
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
