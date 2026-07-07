use std::collections::BTreeMap;

use kat_rs_datasource::TraceDatasource;
use serde_json::Value;

use crate::{
    api::{EvidenceRecordDto, EvidenceRefDto},
    error::ApiError,
};

use super::model::{MetricSpec, RefSpec, SummariesResource};

pub async fn build_evidence(
    datasource: &TraceDatasource,
    resource_coord: &str,
    summaries: &SummariesResource,
) -> Result<Vec<EvidenceRecordDto>, ApiError> {
    let mut records = Vec::new();
    for spec in &summaries.summary.evidence {
        let mut metrics = BTreeMap::new();
        for (name, metric) in &spec.metrics {
            metrics.insert(name.clone(), metric_value(datasource, metric).await?);
        }

        let mut refs = Vec::new();
        for reference in &spec.refs {
            refs.push(reference_rows(datasource, reference).await?);
        }

        records.push(EvidenceRecordDto {
            id: spec.id.clone(),
            fact: spec.fact.clone(),
            metrics,
            refs,
            producing_step: resource_coord.to_string(),
        });
    }
    Ok(records)
}

async fn metric_value(
    datasource: &TraceDatasource,
    metric: &MetricSpec,
) -> Result<Value, ApiError> {
    let sql = match (metric.aggregate.as_str(), metric.column.as_deref()) {
        ("row_count", None) => format!("select count(*) as value from {}", metric.table),
        ("max", Some(column)) => format!("select max({column}) as value from {}", metric.table),
        ("sum", Some(column)) => format!("select sum({column}) as value from {}", metric.table),
        ("count_distinct", Some(column)) => {
            format!("select count(distinct {column}) as value from {}", metric.table)
        }
        _ => {
            return Err(ApiError::validation(format!(
                "unsupported evidence metric aggregate {}",
                metric.aggregate
            )));
        }
    };

    let rows = datasource
        .query_json_rows(&sql)
        .await
        .map_err(|error| ApiError::query_failed(format!("{error:#}")))?;

    Ok(rows
        .first()
        .and_then(|row| row.get("value"))
        .cloned()
        .unwrap_or(Value::Null))
}

async fn reference_rows(
    datasource: &TraceDatasource,
    reference: &RefSpec,
) -> Result<EvidenceRefDto, ApiError> {
    let columns = if reference.columns.is_empty() {
        "*".to_string()
    } else {
        reference.columns.join(", ")
    };
    let order_by = if reference.order_by.is_empty() {
        String::new()
    } else {
        let clauses = reference
            .order_by
            .iter()
            .map(|order| format!("{} {}", order.column, order.direction))
            .collect::<Vec<_>>()
            .join(", ");
        format!(" order by {clauses}")
    };
    let limit = reference
        .max_rows
        .map(|limit| format!(" limit {limit}"))
        .unwrap_or_default();
    let sql = format!("select {columns} from {}{order_by}{limit}", reference.table);
    let rows = datasource
        .query_json_rows(&sql)
        .await
        .map_err(|error| ApiError::query_failed(format!("{error:#}")))?;

    Ok(EvidenceRefDto {
        table: reference.table.clone(),
        rows,
    })
}
