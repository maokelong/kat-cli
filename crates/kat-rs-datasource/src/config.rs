//! Defines the datasource input contract used before a query engine is built.

use std::{path::PathBuf, str::FromStr};

use anyhow::{Result, bail};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataSourceType {
    Hitrace,
}

impl DataSourceType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hitrace => "hitrace",
        }
    }
}

impl FromStr for DataSourceType {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "hitrace" => Ok(Self::Hitrace),
            other => bail!("unsupported datasource type: {other}"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataSourceConfig {
    pub source_type: DataSourceType,
    pub path: PathBuf,
}

impl DataSourceConfig {
    pub fn new(source_type: DataSourceType, path: impl Into<PathBuf>) -> Self {
        Self {
            source_type,
            path: path.into(),
        }
    }

    pub fn hitrace(path: impl Into<PathBuf>) -> Self {
        Self::new(DataSourceType::Hitrace, path)
    }
}
