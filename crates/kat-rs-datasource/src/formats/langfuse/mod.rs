//! Langfuse legacy blob export format adapter.

use std::path::Path;

pub(crate) const LANGFUSE_OBSERVATIONS_TABLE: &str = "langfuse_observations";
pub(crate) const LANGFUSE_TRACES_TABLE: &str = "langfuse_traces";

pub(crate) struct LangfuseJsonTable<'a> {
    pub(crate) name: &'static str,
    pub(crate) path: &'a Path,
}

pub(crate) fn legacy_json_tables<'a>(
    observations_path: &'a Path,
    traces_path: &'a Path,
) -> [LangfuseJsonTable<'a>; 2] {
    [
        LangfuseJsonTable {
            name: LANGFUSE_OBSERVATIONS_TABLE,
            path: observations_path,
        },
        LangfuseJsonTable {
            name: LANGFUSE_TRACES_TABLE,
            path: traces_path,
        },
    ]
}
