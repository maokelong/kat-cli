use std::{collections::BTreeSet, fs, ops::ControlFlow, path::Path};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};
use sqlparser::{
    ast::{ObjectName, visit_relations},
    dialect::SQLiteDialect,
    parser::Parser,
};

use crate::trace_runtime::{
    manifest::{InputSpec, PipelineStep, ProbeManifest, QueryMode, QueryWindowStep},
    operators::{OperatorInput, run_operator},
    query_client::{QueryClient, QueryWindowMode, QueryWindowRequest},
};

pub fn run_manifest(
    manifest: &ProbeManifest,
    probe_dir: &Path,
    params: Value,
    client: &mut dyn QueryClient,
) -> Result<Value> {
    let params = normalize_params(manifest, params)?;
    let mut rows = Vec::new();
    let mut created_views = BTreeSet::new();

    for step in &manifest.pipeline {
        match step {
            PipelineStep::CreateView(step) => {
                let path = probe_dir.join(&step.sql);
                let sql = fs::read_to_string(&path)
                    .with_context(|| format!("failed to read {}", path.display()))?;
                let sql = render_sql(&sql, &params)
                    .with_context(|| format!("failed to render {}", path.display()))?;
                validate_create_view_sql(manifest, &sql, &created_views).with_context(|| {
                    format!("view `{}` violates safety.allowed_tables", step.name)
                })?;
                client
                    .create_view(&step.name, &sql)
                    .with_context(|| format!("failed to create view `{}`", step.name))?;
                created_views.insert(normalize_identifier(&step.name));
            }
            PipelineStep::QueryWindow(step) => {
                rows = client
                    .query_window(query_window_request(step, &params)?)
                    .with_context(|| format!("failed to query `{}`", step.target))?;
            }
            PipelineStep::Operator(step) => {
                return run_operator(OperatorInput {
                    name: &step.name,
                    schema: &manifest.outputs.schema,
                    params,
                    rows,
                });
            }
        }
    }

    bail!("probe `{}` pipeline did not produce evidence", manifest.id)
}

fn normalize_params(manifest: &ProbeManifest, params: Value) -> Result<Value> {
    let input = params
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("probe params must be a JSON object"))?;
    let mut normalized = input.clone();

    for (name, spec) in &manifest.inputs {
        if !has_non_null_value(&normalized, name) {
            if let Some(value) = spec.aliases.iter().find_map(|alias| input.get(alias)) {
                normalized.insert(name.clone(), value.clone());
            }
        }

        if !has_non_null_value(&normalized, name) {
            if let Some(default) = &spec.default {
                normalized.insert(name.clone(), default.clone());
            }
        }

        if spec.required && !has_non_null_value(&normalized, name) {
            bail!("missing required probe input `{name}`");
        }

        if let Some(value) = normalized.get(name).filter(|value| !value.is_null()) {
            let value = normalize_input_value(name, spec, value)?;
            normalized.insert(name.clone(), value);
        }
    }

    enforce_max_rows_limit(manifest, &normalized)?;

    Ok(Value::Object(normalized))
}

fn has_non_null_value(params: &Map<String, Value>, name: &str) -> bool {
    params.get(name).is_some_and(|value| !value.is_null())
}

fn normalize_input_value(name: &str, spec: &InputSpec, value: &Value) -> Result<Value> {
    match spec.value_type.as_str() {
        "number" | "timestamp" | "duration" | "limit" => {
            integer_value(name, value).map(Value::from)
        }
        "array" | "list" => {
            let values = value
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("probe input `{name}` must be an array"))?;
            for item in values {
                match item {
                    Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
                    Value::Array(_) | Value::Object(_) => {
                        bail!("probe input `{name}` must contain only scalar values")
                    }
                }
            }
            Ok(value.clone())
        }
        "object" => {
            if value.is_object() {
                Ok(value.clone())
            } else {
                bail!("probe input `{name}` must be an object")
            }
        }
        "path" | "string" => match value {
            Value::String(_) | Value::Number(_) | Value::Bool(_) => Ok(value.clone()),
            _ => bail!("probe input `{name}` must be a scalar value"),
        },
        other => bail!("unsupported probe input type `{other}` for `{name}`"),
    }
}

fn enforce_max_rows_limit(manifest: &ProbeManifest, params: &Map<String, Value>) -> Result<()> {
    let Some(limit) = manifest.safety.max_rows else {
        return Ok(());
    };
    let Some(value) = params.get("max_rows").filter(|value| !value.is_null()) else {
        return Ok(());
    };

    let max_rows = value_as_u32(value).context("invalid max_rows")?;
    if max_rows > limit {
        bail!("max_rows {max_rows} exceeds safety.max_rows {limit}");
    }

    Ok(())
}

fn integer_value(name: &str, value: &Value) -> Result<i64> {
    match value {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| value.try_into().ok()))
            .ok_or_else(|| anyhow::anyhow!("probe input `{name}` must be an integer")),
        Value::String(value) => value
            .parse::<i64>()
            .with_context(|| format!("probe input `{name}` must be an integer")),
        _ => bail!("probe input `{name}` must be an integer"),
    }
}

fn render_sql(template: &str, params: &Value) -> Result<String> {
    let mut rendered = String::with_capacity(template.len());
    let mut rest = template;

    while let Some(start) = rest.find("${") {
        rendered.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find('}') else {
            bail!("unterminated SQL placeholder");
        };
        let name = after_start[..end].trim();
        rendered.push_str(&render_sql_value(param_value(params, name))?);
        rest = &after_start[end + 1..];
    }

    rendered.push_str(rest);
    Ok(rendered)
}

fn render_sql_value(value: Option<&Value>) -> Result<String> {
    match value {
        None | Some(Value::Null) => Ok("NULL".to_string()),
        Some(Value::Bool(value)) => Ok(i32::from(*value).to_string()),
        Some(Value::Number(value)) => Ok(value.to_string()),
        Some(Value::String(value)) => Ok(escape_string_literal(value)),
        Some(Value::Array(values)) => {
            if values.is_empty() {
                return Ok("NULL".to_string());
            }

            values
                .iter()
                .map(sql_literal)
                .collect::<Result<Vec<_>>>()
                .map(|values| values.join(", "))
        }
        Some(Value::Object(_)) => bail!("object values are unsupported in SQL templates"),
    }
}

fn sql_literal(value: &Value) -> Result<String> {
    match value {
        Value::Null => Ok("NULL".to_string()),
        Value::Bool(value) => Ok(i32::from(*value).to_string()),
        Value::Number(value) => Ok(value.to_string()),
        Value::String(value) => Ok(format!("'{}'", escape_string_literal(value))),
        Value::Array(_) | Value::Object(_) => {
            bail!("nested values are unsupported in SQL literal lists")
        }
    }
}

fn escape_string_literal(value: &str) -> String {
    value.replace('\'', "''")
}

fn query_window_request(step: &QueryWindowStep, params: &Value) -> Result<QueryWindowRequest> {
    Ok(QueryWindowRequest {
        target: step.target.clone(),
        mode: query_window_mode(step.mode),
        time_column: step.time_column.clone(),
        duration_column: step.duration_column.clone(),
        start_ts: param_value(params, "start_ts").and_then(value_as_i64),
        end_ts: param_value(params, "end_ts").and_then(value_as_i64),
        filters: query_filters(step, params)?,
        limit: query_limit(step.limit.as_deref(), params)?,
    })
}

fn query_window_mode(mode: QueryMode) -> QueryWindowMode {
    match mode {
        QueryMode::Window => QueryWindowMode::Window,
        QueryMode::Full => QueryWindowMode::Full,
        QueryMode::Metadata => QueryWindowMode::Metadata,
    }
}

fn query_filters(step: &QueryWindowStep, params: &Value) -> Result<Vec<(String, Value)>> {
    step.filters
        .iter()
        .map(|(column, expr)| Ok((column.clone(), query_expr_value(expr, params)?)))
        .collect()
}

fn query_expr_value(expr: &str, params: &Value) -> Result<Value> {
    let expr = expr.trim();
    if let Some(name) = expr.strip_prefix("input.") {
        return Ok(param_value(params, name).cloned().unwrap_or(Value::Null));
    }

    if let Ok(value) = expr.parse::<i64>() {
        return Ok(Value::from(value));
    }

    Ok(Value::String(expr.to_string()))
}

fn query_limit(expr: Option<&str>, params: &Value) -> Result<Option<u32>> {
    let Some(expr) = expr.map(str::trim).filter(|expr| !expr.is_empty()) else {
        return Ok(None);
    };

    let value = if let Some(name) = expr.strip_prefix("input.") {
        match param_value(params, name) {
            Some(value) if !value.is_null() => return value_as_u32(value).map(Some),
            _ => return Ok(None),
        }
    } else {
        expr.parse::<u32>()
            .with_context(|| format!("invalid query limit `{expr}`"))?
    };

    Ok(Some(value))
}

fn param_value<'a>(params: &'a Value, name: &str) -> Option<&'a Value> {
    let name = name.strip_prefix("input.").unwrap_or(name);
    params.get(name)
}

fn value_as_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| value.try_into().ok())),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn value_as_u32(value: &Value) -> Result<u32> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .and_then(|value| value.try_into().ok())
            .or_else(|| {
                number.as_i64().and_then(|value| {
                    if value >= 0 {
                        value.try_into().ok()
                    } else {
                        None
                    }
                })
            })
            .ok_or_else(|| anyhow::anyhow!("query limit must be a non-negative integer")),
        Value::String(value) => value
            .parse()
            .with_context(|| format!("invalid query limit `{value}`")),
        _ => bail!("query limit must be a number"),
    }
}

fn validate_create_view_sql(
    manifest: &ProbeManifest,
    sql: &str,
    created_views: &BTreeSet<String>,
) -> Result<()> {
    if manifest.safety.allowed_tables.is_empty() {
        return Ok(());
    }

    let mut allowed = manifest
        .safety
        .allowed_tables
        .iter()
        .map(|table| normalize_identifier(table))
        .collect::<BTreeSet<_>>();
    allowed.extend(created_views.iter().cloned());

    let referenced_tables = referenced_tables(sql)?;
    let disallowed = referenced_tables
        .into_iter()
        .filter(|table| !allowed.contains(table))
        .collect::<Vec<_>>();
    if !disallowed.is_empty() {
        bail!(
            "SQL references tables outside safety.allowed_tables: {}",
            disallowed.join(", ")
        );
    }

    Ok(())
}

fn referenced_tables(sql: &str) -> Result<BTreeSet<String>> {
    let dialect = SQLiteDialect {};
    let statements =
        Parser::parse_sql(&dialect, sql).context("failed to parse SQL for table references")?;
    let mut tables = BTreeSet::new();
    let _: ControlFlow<(), ()> = visit_relations(&statements, |relation| {
        if let Some(table) = relation_name(relation) {
            tables.insert(table);
        }
        ControlFlow::Continue(())
    });

    Ok(tables)
}

fn relation_name(relation: &ObjectName) -> Option<String> {
    relation
        .0
        .last()
        .map(ToString::to_string)
        .map(|name| normalize_identifier(name.trim_matches('"')))
        .filter(|name| !name.is_empty())
}

fn normalize_identifier(identifier: &str) -> String {
    identifier.trim_matches('"').to_ascii_lowercase()
}
