use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub(crate) struct DatasetCatalog {
    pub(crate) tables: Vec<DatasetTable>,
}

impl DatasetCatalog {
    pub(crate) fn new(tables: Vec<DatasetTable>) -> Self {
        Self { tables }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub(crate) struct DatasetTable {
    pub(crate) name: String,
    pub(crate) path: String,
}

impl DatasetTable {
    pub(crate) fn new(name: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
        }
    }
}
