use crate::api::{DatasourceSource, InputFileDto, InputRole};

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
        Self { source, inputs }
    }

    pub fn new_for_tests(source: DatasourceSource, inputs: Vec<InputIdentity>) -> Self {
        Self::new(source, inputs)
    }
}
