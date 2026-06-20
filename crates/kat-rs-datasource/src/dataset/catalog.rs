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
    pub(crate) kind: DatasetTableKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) producer: Option<DatasetTableProducer>,
}

impl DatasetTable {
    pub(crate) fn source(name: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            kind: DatasetTableKind::Source,
            producer: None,
        }
    }

    pub(crate) fn derived(
        name: impl Into<String>,
        path: impl Into<String>,
        pack_ref: impl Into<String>,
        transform_id: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            kind: DatasetTableKind::Derived,
            producer: Some(DatasetTableProducer {
                pack_ref: pack_ref.into(),
                transform_id: transform_id.into(),
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DatasetTableKind {
    Source,
    Derived,
}

impl DatasetTableKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Derived => "derived",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub(crate) struct DatasetTableProducer {
    pub(crate) pack_ref: String,
    pub(crate) transform_id: String,
}
