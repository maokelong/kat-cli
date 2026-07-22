use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use directories::ProjectDirs;

const PROJECT_QUALIFIER: &str = "io.github";
const PROJECT_ORGANIZATION: &str = "maokelong";
const PROJECT_APPLICATION: &str = "kat-rs";

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
    pub fn from_datasets_dir(datasets_dir: impl AsRef<Path>) -> Self {
        Self {
            datasets_dir: datasets_dir.as_ref().to_path_buf(),
        }
    }

    pub fn default_from_env() -> Result<Self> {
        let project_dirs =
            ProjectDirs::from(PROJECT_QUALIFIER, PROJECT_ORGANIZATION, PROJECT_APPLICATION)
                .ok_or_else(|| {
                    anyhow!("home directory is required to resolve default platform data directory")
                })?;

        Ok(Self::from_datasets_dir(
            project_dirs.data_dir().join("datasets"),
        ))
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

    pub fn datasets_dir(&self) -> &Path {
        &self.datasets_dir
    }

    fn resolve_name(&self, name: &str) -> DatasetResolution {
        DatasetResolution {
            path: self.datasets_dir.join(name),
            name: Some(name.to_string()),
        }
    }
}

fn validate_dataset_name(name: &str) -> Result<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err(anyhow!("invalid dataset name: {name:?}"));
    }

    Ok(())
}
