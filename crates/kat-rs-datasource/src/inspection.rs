use crate::{
    DatasetHandle, DatasourceQueryRequest, DatasourceResult, DatasourceService, TraceDatasource,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetUiInspection {
    pub schema_version: String,
    pub dataset_id: String,
    pub source_count: usize,
    pub trace: TraceSummary,
    pub tables: BTreeMap<String, TableInspection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSummary {
    pub trace_id: String,
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
    pub clock_domain: Option<String>,
    pub sources: Vec<SourceSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSummary {
    pub source_id: String,
    pub trace_id: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableInspection {
    pub available: bool,
    pub row_count: usize,
    pub reason: Option<String>,
    pub columns: Vec<ColumnInspection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnInspection {
    pub name: String,
    pub data_type: String,
    pub unit: Option<String>,
    pub nullable: Option<bool>,
}

pub async fn inspect_dataset_for_ui<D>(
    service: &DatasourceService<D>,
    handle: &DatasetHandle,
) -> DatasourceResult<DatasetUiInspection>
where
    D: TraceDatasource,
{
    let base = service.inspect(handle).await?;
    let trace = query_trace_summary(service, handle).await?;
    let mut tables = BTreeMap::new();

    for (table_name, capability) in base.tables {
        let columns = query_table_columns(service, handle, &table_name).await?;
        tables.insert(
            table_name,
            TableInspection {
                available: capability.available,
                row_count: capability.row_count,
                reason: capability.reason,
                columns,
            },
        );
    }

    Ok(DatasetUiInspection {
        schema_version: base.schema_version,
        dataset_id: base.dataset_id,
        source_count: base.source_count,
        trace,
        tables,
    })
}

async fn query_trace_summary<D>(
    service: &DatasourceService<D>,
    handle: &DatasetHandle,
) -> DatasourceResult<TraceSummary>
where
    D: TraceDatasource,
{
    let envelope = service
        .query(
            handle,
            DatasourceQueryRequest::new(
                "SELECT trace_id, start_ts, end_ts, clock_domain FROM trace_bounds LIMIT 1",
            ),
        )
        .await?;
    let row = envelope.rows.first();
    let fallback_trace_id = handle
        .sources
        .first()
        .map(|source| source.trace_id.clone())
        .unwrap_or_default();

    Ok(TraceSummary {
        trace_id: row
            .and_then(|value| value.get("trace_id"))
            .and_then(Value::as_str)
            .unwrap_or(fallback_trace_id.as_str())
            .to_string(),
        start_ts: row
            .and_then(|value| value.get("start_ts"))
            .and_then(Value::as_i64),
        end_ts: row
            .and_then(|value| value.get("end_ts"))
            .and_then(Value::as_i64),
        clock_domain: row
            .and_then(|value| value.get("clock_domain"))
            .and_then(Value::as_str)
            .map(ToString::to_string),
        sources: handle
            .sources
            .iter()
            .map(|source| SourceSummary {
                source_id: source.source_id.clone(),
                trace_id: source.trace_id.clone(),
                path: source.path.clone(),
            })
            .collect(),
    })
}

async fn query_table_columns<D>(
    service: &DatasourceService<D>,
    handle: &DatasetHandle,
    table_name: &str,
) -> DatasourceResult<Vec<ColumnInspection>>
where
    D: TraceDatasource,
{
    let envelope = service
        .query(
            handle,
            DatasourceQueryRequest::new(format!(
                "SELECT * FROM {} LIMIT 1",
                quote_identifier(table_name)
            )),
        )
        .await?;

    Ok(envelope
        .columns
        .into_iter()
        .map(|column| ColumnInspection {
            name: column.name,
            data_type: column.data_type,
            unit: column.unit,
            nullable: column.nullable,
        })
        .collect())
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}
