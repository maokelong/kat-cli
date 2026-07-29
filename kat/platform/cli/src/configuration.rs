use std::{
    fs,
    path::{Path, PathBuf},
};

use clap::{Args, Subcommand};
use miette::Diagnostic;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{SkillRootError, locate_skill_root, response};

#[derive(Args)]
pub(super) struct ConfigArgs {
    #[command(subcommand)]
    operation: ConfigOperation,
}

#[derive(Subcommand)]
enum ConfigOperation {
    Get {
        #[command(subcommand)]
        key: ConfigKey,
    },
    Set {
        #[command(subcommand)]
        value: ConfigValue,
    },
}

#[derive(Subcommand)]
enum ConfigKey {
    DataHome,
}

#[derive(Subcommand)]
enum ConfigValue {
    DataHome { directory: PathBuf },
}

#[derive(Serialize)]
pub(super) struct DataHomeResult {
    data_home: String,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Configuration {
    #[serde(default, deserialize_with = "deserialize_optional_nonnull")]
    data_home: Option<String>,
}

fn deserialize_optional_nonnull<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    String::deserialize(deserializer).map(Some)
}

pub(super) fn execute(arguments: ConfigArgs) -> response::PreparedResponse<DataHomeResult> {
    let result = match arguments.operation {
        ConfigOperation::Get {
            key: ConfigKey::DataHome,
        } => current_data_home(),
        ConfigOperation::Set {
            value: ConfigValue::DataHome { directory },
        } => set_data_home(directory),
    };
    match result {
        Ok(data_home) => response::prepare_success(DataHomeResult {
            data_home: path_text(&data_home).expect("canonical Data Home is Unicode"),
        }),
        Err(error) => response::prepare_cli_failure(miette::Report::new(error)),
    }
}

pub(super) fn requires_existing_configuration(arguments: &ConfigArgs) -> bool {
    !matches!(arguments.operation, ConfigOperation::Set { .. })
}

pub(super) fn validate() -> Result<(), ConfigurationError> {
    configured_data_home().map(|_| ())
}

pub(super) fn configured_data_home() -> Result<Option<PathBuf>, ConfigurationError> {
    let skill_root = locate_skill_root().map_err(ConfigurationError::SkillRoot)?;
    let configuration = read_configuration(&skill_root)?;
    configuration
        .data_home
        .map(PathBuf::from)
        .map(|path| {
            if !path.is_absolute() {
                return Err(ConfigurationError::DataHomeNotAbsolute { path });
            }
            let canonical = canonical_directory(path.clone(), "configured KAT Data Home")?;
            if path != canonical {
                return Err(ConfigurationError::DataHomeNotCanonical { path, canonical });
            }
            Ok(canonical)
        })
        .transpose()
}

pub(super) fn data_home() -> Result<PathBuf, ConfigurationError> {
    Ok(configured_data_home()?.unwrap_or(default_data_home()?))
}

pub(super) fn default_data_home() -> Result<PathBuf, ConfigurationError> {
    directories::ProjectDirs::from("", "", "KAT")
        .map(|directories| directories.data_dir().to_path_buf())
        .ok_or(ConfigurationError::PlatformDataHomeUnavailable)
}

fn current_data_home() -> Result<PathBuf, ConfigurationError> {
    let data_home = configured_data_home()?.unwrap_or(default_data_home()?);
    fs::create_dir_all(&data_home).map_err(|source| ConfigurationError::CreateDataHome {
        path: data_home.clone(),
        source,
    })?;
    canonical_directory(data_home, "KAT Data Home")
}

fn set_data_home(directory: PathBuf) -> Result<PathBuf, ConfigurationError> {
    fs::create_dir_all(&directory).map_err(|source| ConfigurationError::CreateDataHome {
        path: directory.clone(),
        source,
    })?;
    let data_home = canonical_directory(directory, "KAT Data Home")?;
    let skill_root = locate_skill_root().map_err(ConfigurationError::SkillRoot)?;
    let configuration = Configuration {
        data_home: Some(path_text(&data_home)?),
    };
    let bytes =
        serde_json::to_vec(&configuration).map_err(ConfigurationError::EncodeConfiguration)?;
    fs::write(skill_root.join("config.json"), bytes).map_err(|source| {
        ConfigurationError::WriteConfiguration {
            path: skill_root.join("config.json"),
            source,
        }
    })?;
    Ok(data_home)
}

fn read_configuration(skill_root: &Path) -> Result<Configuration, ConfigurationError> {
    let path = skill_root.join("config.json");
    match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|source| ConfigurationError::InvalidConfiguration { path, source }),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(Configuration::default())
        }
        Err(source) => Err(ConfigurationError::ReadConfiguration { path, source }),
    }
}

fn canonical_directory(path: PathBuf, label: &'static str) -> Result<PathBuf, ConfigurationError> {
    let canonical =
        dunce::canonicalize(&path).map_err(|source| ConfigurationError::CanonicalDirectory {
            label,
            path,
            source,
        })?;
    if !canonical.is_dir() {
        return Err(ConfigurationError::NotDirectory {
            label,
            path: canonical,
        });
    }
    Ok(canonical)
}

fn path_text(path: &Path) -> Result<String, ConfigurationError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| ConfigurationError::NonUnicodePath {
            path: path.to_path_buf(),
        })
}

#[derive(Debug, Error, Diagnostic)]
pub(super) enum ConfigurationError {
    #[error("KAT Skill is unavailable")]
    #[diagnostic(help("Run kat from a complete KAT Skill deployment"))]
    SkillRoot(#[source] SkillRootError),
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
    #[error("KAT Configuration is invalid: data_home must be an absolute path, got {path}")]
    DataHomeNotAbsolute { path: PathBuf },
    #[error(
        "KAT Configuration is invalid: data_home must be canonical, got {path} (canonical path: {canonical})"
    )]
    DataHomeNotCanonical { path: PathBuf, canonical: PathBuf },
    #[error("failed to create KAT Data Home directory {path}")]
    CreateDataHome {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to resolve {label} directory {path}")]
    CanonicalDirectory {
        label: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{label} is not a directory: {path}")]
    NotDirectory { label: &'static str, path: PathBuf },
    #[error("KAT Data Home path cannot be represented as native Unicode: {path:?}")]
    NonUnicodePath { path: PathBuf },
    #[error("failed to encode KAT Configuration")]
    EncodeConfiguration(#[source] serde_json::Error),
    #[error("failed to write KAT Configuration {path}")]
    WriteConfiguration {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
