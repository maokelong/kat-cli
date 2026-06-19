use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

#[cfg(any(unix, target_os = "redox"))]
use xdg::BaseDirectories;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DatasetLocator {
    Default,
    Name(String),
    Path(PathBuf),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatasetResolution {
    pub path: PathBuf,
    pub name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatasetStore {
    datasets_dir: PathBuf,
}

impl DatasetStore {
    pub fn from_data_home(data_home: impl AsRef<Path>) -> Self {
        Self {
            datasets_dir: data_home.as_ref().join("kat-rs").join("datasets"),
        }
    }

    pub fn default_from_env() -> Result<Self> {
        Ok(Self::from_data_home(xdg_data_home()?))
    }

    pub fn resolve(&self, locator: &DatasetLocator) -> Result<DatasetResolution> {
        match locator {
            DatasetLocator::Default => Ok(self.resolve_name("default")),
            DatasetLocator::Name(name) => {
                validate_dataset_name(name)?;
                Ok(self.resolve_name(name))
            }
            DatasetLocator::Path(path) => Ok(DatasetResolution {
                path: path.clone(),
                name: None,
            }),
        }
    }

    fn resolve_name(&self, name: &str) -> DatasetResolution {
        DatasetResolution {
            path: self.datasets_dir.join(name),
            name: Some(name.to_string()),
        }
    }
}

#[cfg(any(unix, target_os = "redox"))]
fn xdg_data_home() -> Result<PathBuf> {
    BaseDirectories::new()
        .get_data_home()
        .ok_or_else(|| anyhow!("home directory is required to resolve default XDG data directory"))
}

#[cfg(not(any(unix, target_os = "redox")))]
fn xdg_data_home() -> Result<PathBuf> {
    if let Some(value) = std::env::var_os("XDG_DATA_HOME") {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            return Ok(path);
        }
    }

    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| anyhow!("HOME is required to resolve default XDG data directory"))?;

    Ok(home.join(".local").join("share"))
}

fn validate_dataset_name(name: &str) -> Result<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err(anyhow!("invalid dataset name: {name:?}"));
    }

    Ok(())
}
