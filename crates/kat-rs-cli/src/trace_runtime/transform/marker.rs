use std::collections::BTreeSet;

use anyhow::{Result, bail};
use serde_json::Value;

use crate::trace_runtime::{
    adapter::DatasetAdapter,
    analysis::binding::resolve_template,
    pack::spec::{MarkerSourceSpec, TransformSpec},
};

const REQUIRED_TABLES: [&str; 3] = ["callstack", "thread", "process"];

pub fn run_marker_extract_bracket_fields_transform(
    adapter: &mut dyn DatasetAdapter,
    transform: &TransformSpec,
    params: &Value,
    state: &Value,
) -> Result<()> {
    if transform.kind != "marker.extract_bracket_fields" {
        bail!(
            "transform `{}` is not marker.extract_bracket_fields",
            transform.id
        );
    }
    for table in transform.inputs.table_names() {
        if !adapter.table_exists(table)? {
            bail!(
                "transform `{}` input table does not exist: {table}",
                transform.id
            );
        }
    }
    require_declared_inputs(transform)?;
    require_allowed_tables(transform)?;
    reject_unsupported_config(transform)?;

    let source = transform
        .source
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("marker transform `{}` has no source", transform.id))?;
    require_callstack_name_source(source, &transform.id)?;

    let contains = resolve_string_template(&source.contains, params, state)?;
    if contains.trim().is_empty() {
        bail!(
            "marker transform `{}` source.contains must not resolve to empty",
            transform.id
        );
    }
    let process_name = resolve_process_name_filter(transform, params, state)?;
    let start_key = required_field(transform, "start_ts")?;
    let end_key = required_field(transform, "end_ts")?;
    let vsync_key = required_field(transform, "vsync_id")?;

    let process_filter = process_name
        .as_ref()
        .map(|name| format!("AND p.name = {}", sql_literal(name)))
        .unwrap_or_default();
    let sql = format!(
        "WITH extracted AS (
             SELECT
                 c.id AS callstack_id,
                 COALESCE(c.parent_id, c.id) AS root_callstack_id,
                 t.itid AS itid,
                 t.tid AS tid,
                 p.ipid AS ipid,
                 p.name AS process_name,
                 t.thread_name AS thread_name,
                 extract_bracket_int(c.name, {vsync_key}) AS vsync_id,
                 extract_bracket_int(c.name, {start_key}) AS start_ts,
                 extract_bracket_int(c.name, {end_key}) AS end_ts,
                 c.name AS source_name
             FROM callstack c
             JOIN thread t ON t.itid = c.callid
             JOIN process p ON p.ipid = t.ipid
             WHERE instr(c.name, {contains}) > 0
             {process_filter}
         )
         SELECT
             callstack_id,
             root_callstack_id,
             itid,
             tid,
             ipid,
             process_name,
             thread_name,
             vsync_id,
             start_ts,
             end_ts,
             end_ts - start_ts AS dur_ns,
             source_name AS marker_name
         FROM extracted
         WHERE vsync_id IS NOT NULL
           AND start_ts IS NOT NULL
           AND end_ts IS NOT NULL
         ORDER BY start_ts
         LIMIT 1",
        contains = sql_literal(&contains),
        process_filter = process_filter,
        start_key = sql_literal(start_key),
        end_key = sql_literal(end_key),
        vsync_key = sql_literal(vsync_key),
    );

    adapter.create_derived_table_as(&transform.output.table, &sql)?;
    Ok(())
}

fn require_declared_inputs(transform: &TransformSpec) -> Result<()> {
    let declared = transform
        .inputs
        .table_names()
        .into_iter()
        .collect::<BTreeSet<_>>();
    let required = REQUIRED_TABLES.into_iter().collect::<BTreeSet<_>>();
    if declared != required {
        bail!(
            "marker transform `{}` inputs must exactly declare: {}",
            transform.id,
            REQUIRED_TABLES.join(", ")
        );
    }
    Ok(())
}

fn require_allowed_tables(transform: &TransformSpec) -> Result<()> {
    for required in REQUIRED_TABLES {
        if !transform
            .safety
            .allowed_tables
            .iter()
            .any(|table| table == required)
        {
            bail!(
                "marker transform `{}` requires safety.allowedTables to include {required}",
                transform.id
            );
        }
    }
    Ok(())
}

fn reject_unsupported_config(transform: &TransformSpec) -> Result<()> {
    if !transform.joins.is_empty() {
        bail!(
            "marker transform `{}` does not support custom joins",
            transform.id
        );
    }

    let unsupported_filters = transform
        .filters
        .keys()
        .filter(|key| key.as_str() != "process_name")
        .cloned()
        .collect::<Vec<_>>();
    if !unsupported_filters.is_empty() {
        bail!(
            "marker transform `{}` has unsupported filter keys: {}",
            transform.id,
            unsupported_filters.join(", ")
        );
    }
    Ok(())
}

fn require_callstack_name_source(source: &MarkerSourceSpec, transform_id: &str) -> Result<()> {
    if source.table != "callstack" || source.column != "name" {
        bail!("marker transform `{transform_id}` only supports source callstack.name");
    }
    Ok(())
}

fn required_field<'a>(transform: &'a TransformSpec, output_field: &str) -> Result<&'a str> {
    let key = transform
        .fields
        .get(output_field)
        .map(String::as_str)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "marker transform `{}` requires field `{output_field}`",
                transform.id
            )
        })?;
    let key = key.trim();
    if key.is_empty() {
        bail!(
            "marker transform `{}` field `{output_field}` must not be empty",
            transform.id
        );
    }
    Ok(key)
}

fn resolve_process_name_filter(
    transform: &TransformSpec,
    params: &Value,
    state: &Value,
) -> Result<Option<String>> {
    let Some(value) = transform.filters.get("process_name") else {
        return Ok(None);
    };
    match value {
        Value::String(template) => Ok(Some(resolve_string_template(template, params, state)?)),
        _ => bail!(
            "marker transform `{}` process_name filter must be a string",
            transform.id
        ),
    }
}

fn resolve_string_template(template: &str, params: &Value, state: &Value) -> Result<String> {
    match resolve_template(template, params, state)? {
        Value::String(value) => Ok(value),
        _ => bail!("marker templates must resolve to strings"),
    }
}

fn sql_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
