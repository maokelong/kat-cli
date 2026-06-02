use crate::{
    DatasetHandle, DatasourceError, DatasourceQueryRequest, DatasourceResult, DatasourceService,
    QueryEnvelope, TraceDatasource,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoldenCaseReport {
    pub name: String,
    pub status: String,
    pub kind: String,
    pub sql_path: Option<PathBuf>,
    pub expected_path: PathBuf,
    pub actual: Value,
    pub expected: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoldenSuiteReport {
    pub status: String,
    pub cases: Vec<GoldenCaseReport>,
}

pub async fn run_golden_suite<D>(
    service: &DatasourceService<D>,
    handle: &DatasetHandle,
    suite_dir: &Path,
) -> DatasourceResult<GoldenSuiteReport>
where
    D: TraceDatasource,
{
    let mut cases = Vec::new();

    let inspect_expected_path = suite_dir.join("inspect.expected.json");
    if inspect_expected_path.exists() {
        let expected: Value = serde_json::from_str(&fs::read_to_string(&inspect_expected_path)?)?;
        let inspection = service.inspect(handle).await?;
        let full_actual = serde_json::to_value(inspection)?;
        let actual = project_to_expected_shape(&full_actual, &expected);
        let status = if actual == expected { "ok" } else { "failed" }.to_string();

        cases.push(GoldenCaseReport {
            name: "inspect".to_string(),
            status,
            kind: "inspect".to_string(),
            sql_path: None,
            expected_path: inspect_expected_path,
            actual,
            expected,
        });
    }

    for sql_path in sql_paths(&suite_dir.join("queries"))? {
        let expected_path = sql_path.with_extension("expected.json");
        let expected: Value = serde_json::from_str(&fs::read_to_string(&expected_path)?)?;
        let envelope = query_envelope(service, handle, &sql_path).await?;
        let actual = serde_json::json!({ "rows": envelope.rows });
        let status = if actual == expected { "ok" } else { "failed" }.to_string();

        cases.push(GoldenCaseReport {
            name: case_name(&sql_path),
            status,
            kind: "query".to_string(),
            sql_path: Some(sql_path),
            expected_path,
            actual,
            expected,
        });
    }

    for sql_path in sql_paths(&suite_dir.join("errors"))? {
        let expected_path = sql_path.with_extension("expected.json");
        let expected: Value = serde_json::from_str(&fs::read_to_string(&expected_path)?)?;
        let actual = query_error_actual(service, handle, &sql_path).await?;
        let status = if error_expected_matches(&actual, &expected) {
            "ok"
        } else {
            "failed"
        }
        .to_string();

        cases.push(GoldenCaseReport {
            name: case_name(&sql_path),
            status,
            kind: "error".to_string(),
            sql_path: Some(sql_path),
            expected_path,
            actual,
            expected,
        });
    }

    let status = if cases.iter().all(|case| case.status == "ok") {
        "ok"
    } else {
        "failed"
    };

    Ok(GoldenSuiteReport {
        status: status.to_string(),
        cases,
    })
}

fn sql_paths(dir: &Path) -> DatasourceResult<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut paths = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("sql"))
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

fn case_name(sql_path: &Path) -> String {
    sql_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("unnamed")
        .to_string()
}

async fn query_envelope<D>(
    service: &DatasourceService<D>,
    handle: &DatasetHandle,
    sql_path: &Path,
) -> DatasourceResult<QueryEnvelope>
where
    D: TraceDatasource,
{
    let sql = fs::read_to_string(sql_path)?;
    service
        .query(handle, DatasourceQueryRequest::new(sql))
        .await
}

async fn query_error_actual<D>(
    service: &DatasourceService<D>,
    handle: &DatasetHandle,
    sql_path: &Path,
) -> DatasourceResult<Value>
where
    D: TraceDatasource,
{
    let sql = fs::read_to_string(sql_path)?;
    Ok(
        match service
            .query(handle, DatasourceQueryRequest::new(sql))
            .await
        {
            Ok(envelope) => serde_json::json!({
                "status": envelope.status,
                "diagnostics": envelope.diagnostics,
            }),
            Err(error) => serde_json::json!({
                "status": datasource_error_status(&error),
                "diagnostics": [error.to_string()],
            }),
        },
    )
}

fn datasource_error_status(error: &DatasourceError) -> &'static str {
    match error {
        DatasourceError::UnsupportedSchema(_) => "unsupported_schema",
        DatasourceError::UnsupportedSql(_) => "unsupported_sql",
        DatasourceError::InvalidInput(_) => "invalid_params",
        DatasourceError::Timeout => "timeout",
        DatasourceError::ResultTooLarge(_) => "result_too_large",
        DatasourceError::Engine(_) => "engine_error",
    }
}

fn error_expected_matches(actual: &Value, expected: &Value) -> bool {
    let status_matches =
        expected
            .get("status")
            .and_then(Value::as_str)
            .is_none_or(|expected_status| {
                actual.get("status").and_then(Value::as_str) == Some(expected_status)
            });
    let diagnostics = actual
        .get("diagnostics")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    let diagnostics_match = expected
        .get("diagnostics_contains")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .all(|needle| {
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.contains(needle))
        });

    status_matches && diagnostics_match
}

fn project_to_expected_shape(actual: &Value, expected: &Value) -> Value {
    match (actual, expected) {
        (Value::Object(actual_map), Value::Object(expected_map)) => Value::Object(
            expected_map
                .iter()
                .filter_map(|(key, expected_value)| {
                    actual_map.get(key).map(|actual_value| {
                        (
                            key.clone(),
                            project_to_expected_shape(actual_value, expected_value),
                        )
                    })
                })
                .collect(),
        ),
        _ => actual.clone(),
    }
}
