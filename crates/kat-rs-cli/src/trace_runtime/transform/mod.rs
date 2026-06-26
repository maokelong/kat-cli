use anyhow::{Result, bail};

use crate::trace_runtime::pack::spec::TransformSpec;

pub mod marker;
pub mod payload;
pub mod rules;
pub mod sql;

pub(crate) fn reject_marker_only_config(transform: &TransformSpec, kind: &str) -> Result<()> {
    if transform.source.is_some()
        || !transform.fields.is_empty()
        || !transform.joins.is_empty()
        || !transform.filters.is_empty()
    {
        bail!(
            "{kind} transform `{}` does not support marker-only config fields",
            transform.id
        );
    }
    Ok(())
}
