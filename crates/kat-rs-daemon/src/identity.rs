use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    api::{DatasourceSource, InputFileDto, InputRole},
    error::ApiError,
};

#[derive(Clone, Debug)]
pub struct ResolvedInput {
    pub role: InputRole,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub modified: SystemTime,
    pub modified_at: String,
}

impl ResolvedInput {
    pub fn to_identity(&self) -> InputIdentity {
        InputIdentity::new(
            self.role,
            self.path.to_string_lossy(),
            self.size_bytes,
            self.modified_at.clone(),
        )
    }
}

pub fn resolve_input(role: InputRole, path: impl AsRef<Path>) -> Result<ResolvedInput, ApiError> {
    let path = path.as_ref();
    let canonical_path =
        normalize_canonical_path(std::fs::canonicalize(path).map_err(|error| {
            ApiError::validation(format!(
                "failed to resolve input file {}: {error}",
                path.display()
            ))
        })?);
    let metadata = std::fs::metadata(&canonical_path).map_err(|error| {
        ApiError::validation(format!(
            "failed to read input file metadata {}: {error}",
            canonical_path.display()
        ))
    })?;

    if !metadata.is_file() {
        return Err(ApiError::validation(format!(
            "input path is not a file: {}",
            canonical_path.display()
        )));
    }

    let modified = metadata.modified().map_err(|error| {
        ApiError::validation(format!(
            "failed to read input file modified time {}: {error}",
            canonical_path.display()
        ))
    })?;
    let modified_at = format_system_time(modified)?;

    Ok(ResolvedInput {
        role,
        path: canonical_path,
        size_bytes: metadata.len(),
        modified,
        modified_at,
    })
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct InputIdentity {
    pub role: InputRole,
    pub path: String,
    pub size_bytes: u64,
    pub modified_at: String,
}

impl InputIdentity {
    pub fn new(
        role: InputRole,
        path: impl Into<String>,
        size_bytes: u64,
        modified_at: impl Into<String>,
    ) -> Self {
        Self {
            role,
            path: path.into(),
            size_bytes,
            modified_at: modified_at.into(),
        }
    }

    pub fn new_for_tests(role: InputRole, path: impl Into<String>) -> Self {
        Self::new(role, path, 0, "1970-01-01T00:00:00Z")
    }

    pub fn to_dto(&self) -> InputFileDto {
        InputFileDto {
            role: self.role,
            path: self.path.clone(),
            size_bytes: self.size_bytes,
            modified_at: self.modified_at.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct DatasourceIdentityKey {
    pub source: DatasourceSource,
    pub inputs: Vec<InputIdentity>,
}

impl DatasourceIdentityKey {
    pub fn new(source: DatasourceSource, inputs: Vec<InputIdentity>) -> Self {
        let mut inputs = inputs;
        inputs.sort_by(|left, right| {
            left.role
                .cmp(&right.role)
                .then_with(|| left.path.cmp(&right.path))
        });

        Self { source, inputs }
    }

    pub fn new_for_tests(source: DatasourceSource, inputs: Vec<InputIdentity>) -> Self {
        Self::new(source, inputs)
    }
}

fn format_system_time(time: SystemTime) -> Result<String, ApiError> {
    OffsetDateTime::from(time)
        .format(&Rfc3339)
        .map_err(|_| ApiError::internal("failed to format input modified time"))
}

#[cfg(windows)]
fn normalize_canonical_path(path: PathBuf) -> PathBuf {
    let path_text = path.to_string_lossy();

    if let Some(rest) = path_text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }

    if let Some(rest) = path_text.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }

    path
}

#[cfg(not(windows))]
fn normalize_canonical_path(path: PathBuf) -> PathBuf {
    path
}
