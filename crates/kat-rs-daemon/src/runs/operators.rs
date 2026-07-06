use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    sync::Arc,
};

use arrow_array::{
    Array, BooleanArray, Float32Array, Float64Array, Int32Array, Int64Array, LargeStringArray,
    RecordBatch, StringArray, StringViewArray, UInt32Array, UInt64Array,
};
use arrow_schema::{DataType, Field, Schema};
use kat_rs_datasource::TraceDatasource;
use regex::Regex;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::error::ApiError;

use super::{
    context::{ContextStore, ContextValue},
    model::RunStepRecord,
    render::render_template,
    resources::{
        BriefOrderBy, BriefSection, EvidenceSpec, Flow, FlowStep, GrepResource, Manifest,
        MetricSpec, OrderBy, Pack, PublishSpec, QueryResource, RefSpec, ResourceRoot,
        SummaryResource,
    },
};

pub struct ExecutionState {
    pub datasource: TraceDatasource,
    pub context: ContextStore,
    pub steps: Vec<RunStepRecord>,
    pub diagnostics: Vec<Value>,
    pub evidence: Vec<Value>,
    pub brief_sections: Vec<Value>,
}

impl ExecutionState {
    pub fn new(datasource: TraceDatasource) -> Self {
        Self {
            datasource,
            context: ContextStore::new(),
            steps: Vec::new(),
            diagnostics: Vec::new(),
            evidence: Vec::new(),
            brief_sections: Vec::new(),
        }
    }
}

pub fn row_count_sql(table: &str) -> String {
    format!("select count(*) as row_count from {table}")
}

pub fn append_table_sql(target: &str, source: &str) -> String {
    format!("select * from {target} union all select * from {source}")
}

pub fn execute_flow<'a>(
    root: &'a ResourceRoot,
    manifest: &'a Manifest,
    pack: &'a Pack,
    flow: &'a Flow,
    state: &'a mut ExecutionState,
) -> Pin<Box<dyn Future<Output = Result<(), ApiError>> + Send + 'a>> {
    Box::pin(async move {
        for step in &flow.steps {
            execute_step(root, manifest, pack, step, state).await?;
        }

        Ok(())
    })
}

pub async fn build_brief_sections(
    root: &ResourceRoot,
    pack: &super::resources::LoadedYaml<Pack>,
    state: &mut ExecutionState,
) -> Result<(), ApiError> {
    let brief = root.load_pack_brief(pack)?;
    let mut sections = Vec::new();

    for section in brief.value.sections {
        sections.push(build_brief_section(&section, state).await?);
    }

    state.brief_sections = sections;
    Ok(())
}

fn execute_step<'a>(
    root: &'a ResourceRoot,
    manifest: &'a Manifest,
    pack: &'a Pack,
    step: &'a FlowStep,
    state: &'a mut ExecutionState,
) -> Pin<Box<dyn Future<Output = Result<(), ApiError>> + Send + 'a>> {
    Box::pin(async move {
        match step.uses.as_str() {
            "flow" => execute_flow_step(root, manifest, pack, step, state).await,
            "grep" => execute_grep_step(root, manifest, step, state).await,
            "query" => execute_query_step(root, manifest, step, state).await,
            "branch" => execute_branch_step(root, manifest, pack, step, state).await,
            "loop" => execute_loop_step(root, manifest, pack, step, state).await,
            "summaries" => execute_summaries_step(root, manifest, pack, step, state).await,
            other => Err(ApiError::validation(format!(
                "unsupported run step operator: {other}"
            ))),
        }
    })
}

async fn execute_flow_step(
    root: &ResourceRoot,
    manifest: &Manifest,
    pack: &Pack,
    step: &FlowStep,
    state: &mut ExecutionState,
) -> Result<(), ApiError> {
    let resource = required_resource(step)?;
    let flow_ref = pack
        .imports
        .flows
        .get(resource)
        .map(String::as_str)
        .unwrap_or(resource);
    let flow = root.load_flow_resource(manifest, flow_ref)?;

    execute_flow(root, manifest, pack, &flow.value, state).await?;
    state.steps.push(RunStepRecord::completed(
        &step.id,
        &step.uses,
        step.output.clone(),
        None,
    ));
    Ok(())
}

pub async fn execute_query_step(
    root: &ResourceRoot,
    manifest: &Manifest,
    step: &FlowStep,
    state: &mut ExecutionState,
) -> Result<(), ApiError> {
    let resource = root.load_query_resource(manifest, required_resource(step)?)?;
    let row_count = execute_query_resource(&resource.value, state).await?;

    state.steps.push(RunStepRecord::completed(
        &step.id,
        &step.uses,
        Some(resource.value.output.table.clone()),
        Some(row_count),
    ));
    Ok(())
}

pub async fn execute_grep_step(
    root: &ResourceRoot,
    manifest: &Manifest,
    step: &FlowStep,
    state: &mut ExecutionState,
) -> Result<(), ApiError> {
    let resource = root.load_grep_resource(manifest, required_resource(step)?)?;
    let row_count = execute_grep_resource(&resource.value, state).await?;

    state.steps.push(RunStepRecord::completed(
        &step.id,
        &step.uses,
        Some(resource.value.output.table.clone()),
        Some(row_count),
    ));
    Ok(())
}

pub async fn execute_branch_step(
    root: &ResourceRoot,
    manifest: &Manifest,
    pack: &Pack,
    step: &FlowStep,
    state: &mut ExecutionState,
) -> Result<(), ApiError> {
    let config: BranchConfig = parse_step_extra(step)?;
    let actual = table_row_count(&state.datasource, &config.when.row_count.table).await?;
    let selected_steps = if actual == config.when.row_count.equals {
        &config.then_steps
    } else {
        &config.else_steps
    };

    for selected_step in selected_steps {
        execute_step(root, manifest, pack, selected_step, state).await?;
    }

    state.steps.push(RunStepRecord::completed(
        &step.id,
        &step.uses,
        None,
        Some(actual),
    ));
    Ok(())
}

pub async fn execute_loop_step(
    root: &ResourceRoot,
    manifest: &Manifest,
    pack: &Pack,
    step: &FlowStep,
    state: &mut ExecutionState,
) -> Result<(), ApiError> {
    let config: LoopConfig = parse_step_extra(step)?;
    let max_iterations = context_usize(&state.context, &config.max_iterations.slot)?;
    let mut initialized = HashSet::new();
    let mut iterations = 0;

    for _ in 0..max_iterations {
        iterations += 1;
        for body_step in &config.body {
            execute_step(root, manifest, pack, body_step, state).await?;
        }

        for (target, accumulator) in &config.accumulators {
            if accumulator.kind != "table" {
                return Err(ApiError::validation(format!(
                    "unsupported loop accumulator kind for {target}: {}",
                    accumulator.kind
                )));
            }
            append_accumulator_table(state, target, &accumulator.append_from, &mut initialized)
                .await?;
        }

        if table_row_count(&state.datasource, "next_anchor_rows").await? == 0 {
            break;
        }
    }

    state.steps.push(RunStepRecord::completed(
        &step.id,
        &step.uses,
        None,
        Some(iterations),
    ));
    Ok(())
}

pub async fn execute_summaries_step(
    root: &ResourceRoot,
    manifest: &Manifest,
    pack: &Pack,
    step: &FlowStep,
    state: &mut ExecutionState,
) -> Result<(), ApiError> {
    let resource = required_resource(step)?;
    let summary_ref = pack
        .imports
        .summaries
        .get(resource)
        .map(String::as_str)
        .unwrap_or(resource);
    let resource = root.load_summary_resource(manifest, summary_ref)?;
    let evidence = execute_summary_resource(&resource.value, state).await?;
    let row_count = evidence.len();

    state.evidence.extend(evidence);
    state.steps.push(RunStepRecord::completed(
        &step.id,
        &step.uses,
        step.output.clone(),
        Some(row_count),
    ));
    Ok(())
}

async fn execute_query_resource(
    resource: &QueryResource,
    state: &mut ExecutionState,
) -> Result<usize, ApiError> {
    let sql = render_template(&resource.sql, &state.context)?;
    let batches = state
        .datasource
        .query_batches(&sql)
        .await
        .map_err(|error| ApiError::query_failed(format!("{error:#}")))?;
    let row_count = batches.iter().map(RecordBatch::num_rows).sum();
    let rows = batches_to_rows(&batches)?;
    let register_batches = normalize_batches_for_register(&batches, &rows)?;

    replace_run_table(state, &resource.output.table, register_batches).await?;
    publish_first_row(
        &resource.context.publishes,
        rows.first(),
        &mut state.context,
        &resource.id,
    )?;

    Ok(row_count)
}

async fn execute_grep_resource(
    resource: &GrepResource,
    state: &mut ExecutionState,
) -> Result<usize, ApiError> {
    let query_columns = grep_query_columns(resource);
    let sql = format!(
        "select {} from {}{}",
        query_columns
            .iter()
            .map(|column| column.sql.clone())
            .collect::<Vec<_>>()
            .join(", "),
        sql_ident(&resource.target.table)?,
        grep_where_sql(resource, &state.context)?
    );
    let batches = state
        .datasource
        .query_batches(&sql)
        .await
        .map_err(|error| ApiError::query_failed(format!("{error:#}")))?;
    let mut rows = batches_to_rows(&batches)?;
    let patterns = resource
        .patterns
        .iter()
        .map(|pattern| {
            let rendered = render_template(&pattern.value, &state.context)?;
            Regex::new(&rendered).map_err(|error| {
                ApiError::validation(format!(
                    "invalid regex pattern in grep resource {}: {error}",
                    resource.id
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    rows.retain(|row| {
        predicates_match(row, &resource.predicates, &state.context)
            && patterns_match(row, &resource.target.columns, &patterns)
    });
    sort_rows(&mut rows, &resource.order_by);
    if let Some(limit) = resource.limit {
        rows.truncate(limit);
    }

    let output_rows = rows
        .into_iter()
        .map(|row| project_row(row, &resource.output.columns))
        .collect::<Vec<_>>();
    let row_count = output_rows.len();
    let batches = if output_rows.is_empty() {
        empty_grep_batches(resource, state).await?
    } else {
        vec![rows_to_record_batch(
            &resource.output.columns,
            &output_rows,
        )?]
    };

    replace_run_table(state, &resource.output.table, batches).await?;
    publish_first_row(
        &resource.context.publishes,
        output_rows.first(),
        &mut state.context,
        &resource.id,
    )?;

    Ok(row_count)
}

async fn append_accumulator_table(
    state: &mut ExecutionState,
    target: &str,
    source: &str,
    initialized: &mut HashSet<String>,
) -> Result<(), ApiError> {
    let sql = if initialized.contains(target) {
        append_table_sql(&sql_ident(target)?, &sql_ident(source)?)
    } else {
        initialized.insert(target.to_owned());
        format!("select * from {}", sql_ident(source)?)
    };
    let batches = state
        .datasource
        .query_batches(&sql)
        .await
        .map_err(|error| ApiError::query_failed(format!("{error:#}")))?;
    let rows = batches_to_rows(&batches)?;
    let batches = normalize_batches_for_register(&batches, &rows)?;

    replace_run_table(state, target, batches).await?;
    Ok(())
}

async fn replace_run_table(
    state: &mut ExecutionState,
    table: &str,
    batches: Vec<RecordBatch>,
) -> Result<(), ApiError> {
    let table_ident = sql_ident(table)?;
    let drop_sql = format!("drop table if exists {table_ident}");

    state
        .datasource
        .query_batches(&drop_sql)
        .await
        .map_err(|error| ApiError::query_failed(format!("{error:#}")))?;
    state
        .datasource
        .register_record_batches(table, batches)
        .map_err(|error| {
            ApiError::query_failed(format!("failed to register table {table}: {error:#}"))
        })?;

    Ok(())
}

async fn execute_summary_resource(
    resource: &SummaryResource,
    state: &ExecutionState,
) -> Result<Vec<Value>, ApiError> {
    let mut evidence = Vec::new();

    for spec in &resource.summary.evidence {
        evidence.push(build_evidence(spec, state).await?);
    }

    Ok(evidence)
}

async fn build_evidence(spec: &EvidenceSpec, state: &ExecutionState) -> Result<Value, ApiError> {
    let mut metrics = Map::new();
    let mut metric_names = spec.metrics.keys().cloned().collect::<Vec<_>>();

    metric_names.sort();
    for name in metric_names {
        let metric = spec
            .metrics
            .get(&name)
            .expect("metric name came from metrics map");
        metrics.insert(name, compute_metric(metric, state).await?);
    }

    let mut refs = Vec::new();
    for ref_spec in &spec.refs {
        refs.push(build_ref(ref_spec, state).await?);
    }

    Ok(json!({
        "id": spec.id,
        "fact": spec.fact,
        "metrics": Value::Object(metrics),
        "refs": refs,
    }))
}

async fn compute_metric(metric: &MetricSpec, state: &ExecutionState) -> Result<Value, ApiError> {
    let table = sql_ident(&metric.table)?;
    let sql = match metric.aggregate.as_str() {
        "row_count" => row_count_sql(&table),
        "max" => format!(
            "select max({}) as value from {table}",
            sql_ident(required_column(metric)?)?
        ),
        "sum" => format!(
            "select sum({}) as value from {table}",
            sql_ident(required_column(metric)?)?
        ),
        "count_distinct" => format!(
            "select count(distinct {}) as value from {table}",
            sql_ident(required_column(metric)?)?
        ),
        other => {
            return Err(ApiError::validation(format!(
                "unsupported summary aggregate: {other}"
            )));
        }
    };
    let rows = query_rows(&state.datasource, &sql).await?;
    let row = rows
        .first()
        .ok_or_else(|| ApiError::query_failed("summary metric query returned no rows"))?;
    let column = if metric.aggregate == "row_count" {
        "row_count"
    } else {
        "value"
    };

    Ok(row.get(column).cloned().unwrap_or(Value::Null))
}

async fn build_ref(ref_spec: &RefSpec, state: &ExecutionState) -> Result<Value, ApiError> {
    let columns = if ref_spec.columns.is_empty() {
        "*".to_owned()
    } else {
        ref_spec
            .columns
            .iter()
            .map(|column| sql_ident(column))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ")
    };
    let mut sql = format!("select {columns} from {}", sql_ident(&ref_spec.table)?);

    sql.push_str(&order_by_sql(&ref_spec.order_by)?);
    if let Some(max_rows) = ref_spec.max_rows {
        sql.push_str(&format!(" limit {max_rows}"));
    }

    Ok(json!({
        "table": ref_spec.table,
        "rows": query_rows(&state.datasource, &sql).await?,
    }))
}

async fn build_brief_section(
    section: &BriefSection,
    state: &ExecutionState,
) -> Result<Value, ApiError> {
    let columns = if section.include.is_empty() {
        "*".to_owned()
    } else {
        section
            .include
            .iter()
            .map(|column| sql_ident(column))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ")
    };
    let mut sql = format!("select {columns} from {}", sql_ident(&section.from_table)?);

    if let Some(order_by) = &section.order_by {
        sql.push_str(&brief_order_by_sql(order_by)?);
    }
    if let Some(limit) = section.limit {
        sql.push_str(&format!(" limit {limit}"));
    }

    Ok(json!({
        "id": section.id,
        "from": section.from_table,
        "rows": query_rows(&state.datasource, &sql).await?,
    }))
}

async fn table_row_count(datasource: &TraceDatasource, table: &str) -> Result<usize, ApiError> {
    let table = sql_ident(table)?;
    let rows = query_rows(datasource, &row_count_sql(&table)).await?;
    let row_count = rows
        .first()
        .and_then(|row| row.get("row_count"))
        .and_then(value_as_i64)
        .ok_or_else(|| ApiError::query_failed("row count query did not return row_count"))?;

    usize::try_from(row_count)
        .map_err(|_| ApiError::query_failed(format!("row count is out of range: {row_count}")))
}

async fn query_rows(
    datasource: &TraceDatasource,
    sql: &str,
) -> Result<Vec<Map<String, Value>>, ApiError> {
    let batches = datasource
        .query_batches(sql)
        .await
        .map_err(|error| ApiError::query_failed(format!("{error:#}")))?;

    batches_to_rows(&batches)
}

fn publish_first_row(
    publishes: &HashMap<String, PublishSpec>,
    row: Option<&Map<String, Value>>,
    context: &mut ContextStore,
    producing_step: &str,
) -> Result<(), ApiError> {
    if publishes.is_empty() {
        return Ok(());
    }
    let row = row.ok_or_else(|| {
        ApiError::validation(format!(
            "resource {producing_step} produced no rows for context publishes"
        ))
    })?;

    for (slot, publish) in publishes {
        match publish.carrier.as_str() {
            "scalar" => {
                let column = publish.from.column.as_deref().ok_or_else(|| {
                    ApiError::validation(format!("scalar publish {slot} is missing from.column"))
                })?;
                context.publish_scalar(
                    slot,
                    row.get(column).cloned().unwrap_or(Value::Null),
                    producing_step,
                )?;
            }
            "interval" => {
                let start_column = publish.from.start_column.as_deref().ok_or_else(|| {
                    ApiError::validation(format!(
                        "interval publish {slot} is missing from.start_column"
                    ))
                })?;
                let end_column = publish.from.end_column.as_deref().ok_or_else(|| {
                    ApiError::validation(format!(
                        "interval publish {slot} is missing from.end_column"
                    ))
                })?;
                let start = row
                    .get(start_column)
                    .and_then(value_as_i64)
                    .ok_or_else(|| {
                        ApiError::validation(format!(
                            "interval publish {slot} start column {start_column} is not an integer"
                        ))
                    })?;
                let end = row.get(end_column).and_then(value_as_i64).ok_or_else(|| {
                    ApiError::validation(format!(
                        "interval publish {slot} end column {end_column} is not an integer"
                    ))
                })?;

                context.publish_interval(slot, start, end, producing_step)?;
            }
            other => {
                return Err(ApiError::validation(format!(
                    "unsupported context carrier for {slot}: {other}"
                )));
            }
        }
    }

    Ok(())
}

fn grep_query_columns(resource: &GrepResource) -> Vec<QueryColumn> {
    let mut aliases = HashSet::new();
    let mut columns = Vec::new();

    add_query_column(
        &mut columns,
        &mut aliases,
        "source_row_id",
        "id as source_row_id",
    );
    for column in resource
        .output
        .columns
        .iter()
        .chain(resource.target.columns.iter())
        .chain(
            resource
                .predicates
                .iter()
                .map(|predicate| &predicate.column),
        )
        .chain(resource.order_by.iter().map(|order| &order.column))
    {
        if column != "source_row_id" {
            add_query_column(&mut columns, &mut aliases, column, column);
        }
    }

    columns
}

fn add_query_column(
    columns: &mut Vec<QueryColumn>,
    aliases: &mut HashSet<String>,
    alias: &str,
    sql: &str,
) {
    if aliases.insert(alias.to_owned()) {
        columns.push(QueryColumn {
            sql: sql.to_owned(),
        });
    }
}

fn predicates_match(
    row: &Map<String, Value>,
    predicates: &[super::resources::GrepPredicate],
    context: &ContextStore,
) -> bool {
    predicates.iter().all(|predicate| {
        let value = row.get(&predicate.column).unwrap_or(&Value::Null);
        if predicate.is_not_null && value.is_null() {
            return false;
        }
        if let Some(expected) = &predicate.equals {
            let Ok(expected) = render_template(expected, context) else {
                return false;
            };
            return value_equals_rendered(value, &expected);
        }

        true
    })
}

fn patterns_match(row: &Map<String, Value>, columns: &[String], patterns: &[Regex]) -> bool {
    patterns.iter().all(|pattern| {
        columns.iter().any(|column| {
            row.get(column)
                .and_then(value_as_str)
                .is_some_and(|value| pattern.is_match(value))
        })
    })
}

fn grep_where_sql(resource: &GrepResource, context: &ContextStore) -> Result<String, ApiError> {
    let mut clauses = Vec::new();

    for predicate in &resource.predicates {
        if predicate.is_not_null {
            clauses.push(format!("{} is not null", sql_ident(&predicate.column)?));
        }
        if let Some(expected) = &predicate.equals {
            let expected = render_template(expected, context)?;
            clauses.push(format!(
                "{} = {}",
                sql_ident(&predicate.column)?,
                sql_literal_from_rendered(&expected)
            ));
        }
    }

    if clauses.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!(" where {}", clauses.join(" and ")))
    }
}

fn sort_rows(rows: &mut [Map<String, Value>], order_by: &[OrderBy]) {
    rows.sort_by(|left, right| {
        for order in order_by {
            let direction = order.direction.as_deref().unwrap_or("asc");
            let ordering = compare_values(left.get(&order.column), right.get(&order.column));
            if ordering != Ordering::Equal {
                return if direction.eq_ignore_ascii_case("desc") {
                    ordering.reverse()
                } else {
                    ordering
                };
            }
        }

        Ordering::Equal
    });
}

fn project_row(row: Map<String, Value>, columns: &[String]) -> Map<String, Value> {
    let mut projected = Map::new();

    for column in columns {
        projected.insert(
            column.clone(),
            row.get(column).cloned().unwrap_or(Value::Null),
        );
    }

    projected
}

async fn empty_grep_batches(
    resource: &GrepResource,
    state: &ExecutionState,
) -> Result<Vec<RecordBatch>, ApiError> {
    let columns = resource
        .output
        .columns
        .iter()
        .map(|column| {
            if column == "source_row_id" {
                Ok("id as source_row_id".to_owned())
            } else {
                sql_ident(column)
            }
        })
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    let sql = format!(
        "select {columns} from {} where 1 = 0",
        sql_ident(&resource.target.table)?
    );

    state
        .datasource
        .query_batches(&sql)
        .await
        .map_err(|error| ApiError::query_failed(format!("{error:#}")))
}

fn rows_to_record_batch(
    columns: &[String],
    rows: &[Map<String, Value>],
) -> Result<RecordBatch, ApiError> {
    let mut fields = Vec::new();
    let mut arrays: Vec<Arc<dyn Array>> = Vec::new();

    for column in columns {
        let data_type = infer_column_type(column, rows);
        fields.push(Field::new(column, data_type.clone(), true));
        arrays.push(values_to_array(column, &data_type, rows)?);
    }

    RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays)
        .map_err(|error| ApiError::query_failed(format!("{error:#}")))
}

fn normalize_batches_for_register(
    batches: &[RecordBatch],
    rows: &[Map<String, Value>],
) -> Result<Vec<RecordBatch>, ApiError> {
    if rows.is_empty() {
        return Ok(batches.to_vec());
    }
    let schema = batches
        .first()
        .ok_or_else(|| ApiError::query_failed("query returned rows without a record batch"))?
        .schema();
    let columns = schema
        .fields()
        .iter()
        .map(|field| field.name().clone())
        .collect::<Vec<_>>();

    Ok(vec![rows_to_record_batch(&columns, rows)?])
}

fn infer_column_type(column: &str, rows: &[Map<String, Value>]) -> DataType {
    rows.iter()
        .filter_map(|row| row.get(column))
        .find_map(|value| match value {
            Value::Bool(_) => Some(DataType::Boolean),
            Value::Number(number) if number.is_i64() || number.is_u64() => Some(DataType::Int64),
            Value::Number(_) => Some(DataType::Float64),
            Value::String(_) => Some(DataType::Utf8),
            Value::Null | Value::Array(_) | Value::Object(_) => None,
        })
        .unwrap_or(DataType::Utf8)
}

fn values_to_array(
    column: &str,
    data_type: &DataType,
    rows: &[Map<String, Value>],
) -> Result<Arc<dyn Array>, ApiError> {
    match data_type {
        DataType::Boolean => Ok(Arc::new(BooleanArray::from(
            rows.iter()
                .map(|row| row.get(column).and_then(Value::as_bool))
                .collect::<Vec<_>>(),
        ))),
        DataType::Int64 => Ok(Arc::new(Int64Array::from(
            rows.iter()
                .map(|row| row.get(column).and_then(value_as_i64))
                .collect::<Vec<_>>(),
        ))),
        DataType::Float64 => Ok(Arc::new(Float64Array::from(
            rows.iter()
                .map(|row| row.get(column).and_then(Value::as_f64))
                .collect::<Vec<_>>(),
        ))),
        DataType::Utf8 => Ok(Arc::new(StringArray::from(
            rows.iter()
                .map(|row| row.get(column).and_then(value_as_str).map(str::to_owned))
                .collect::<Vec<_>>(),
        ))),
        other => Err(ApiError::query_failed(format!(
            "unsupported inferred grep column type for {column}: {other:?}"
        ))),
    }
}

fn batches_to_rows(batches: &[RecordBatch]) -> Result<Vec<Map<String, Value>>, ApiError> {
    let mut rows = Vec::new();

    for batch in batches {
        let schema = batch.schema();
        for row_index in 0..batch.num_rows() {
            let mut row = Map::new();

            for (column_index, field) in schema.fields().iter().enumerate() {
                let column = batch.column(column_index);
                row.insert(field.name().clone(), array_value(column, row_index)?);
            }

            rows.push(row);
        }
    }

    Ok(rows)
}

fn array_value(array: &Arc<dyn Array>, row_index: usize) -> Result<Value, ApiError> {
    if matches!(array.data_type(), DataType::Null) {
        return Ok(Value::Null);
    }
    if array.is_null(row_index) {
        return Ok(Value::Null);
    }
    if let Some(values) = array.as_any().downcast_ref::<Int64Array>() {
        return Ok(json!(values.value(row_index)));
    }
    if let Some(values) = array.as_any().downcast_ref::<Int32Array>() {
        return Ok(json!(values.value(row_index)));
    }
    if let Some(values) = array.as_any().downcast_ref::<UInt64Array>() {
        return Ok(json!(values.value(row_index)));
    }
    if let Some(values) = array.as_any().downcast_ref::<UInt32Array>() {
        return Ok(json!(values.value(row_index)));
    }
    if let Some(values) = array.as_any().downcast_ref::<Float64Array>() {
        return Ok(json!(values.value(row_index)));
    }
    if let Some(values) = array.as_any().downcast_ref::<Float32Array>() {
        return Ok(json!(values.value(row_index)));
    }
    if let Some(values) = array.as_any().downcast_ref::<StringArray>() {
        return Ok(json!(values.value(row_index)));
    }
    if let Some(values) = array.as_any().downcast_ref::<LargeStringArray>() {
        return Ok(json!(values.value(row_index)));
    }
    if let Some(values) = array.as_any().downcast_ref::<StringViewArray>() {
        return Ok(json!(values.value(row_index)));
    }
    if let Some(values) = array.as_any().downcast_ref::<BooleanArray>() {
        return Ok(json!(values.value(row_index)));
    }

    Err(ApiError::query_failed(format!(
        "unsupported query result column type: {:?}",
        array.data_type()
    )))
}

fn context_usize(context: &ContextStore, slot: &str) -> Result<usize, ApiError> {
    match context.value(slot)? {
        ContextValue::Scalar(value) => value
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| ApiError::validation(format!("context slot {slot} is not a usize"))),
        ContextValue::Interval { .. } => Err(ApiError::validation(format!(
            "context slot {slot} is an interval, expected scalar"
        ))),
    }
}

fn required_resource(step: &FlowStep) -> Result<&str, ApiError> {
    step.resource
        .as_deref()
        .ok_or_else(|| ApiError::validation(format!("step {} is missing resource", step.id)))
}

fn required_column(metric: &MetricSpec) -> Result<&str, ApiError> {
    metric.column.as_deref().ok_or_else(|| {
        ApiError::validation(format!(
            "summary aggregate {} requires column",
            metric.aggregate
        ))
    })
}

fn parse_step_extra<T>(step: &FlowStep) -> Result<T, ApiError>
where
    T: for<'de> Deserialize<'de>,
{
    let map = step
        .extra
        .clone()
        .into_iter()
        .collect::<Map<String, Value>>();

    serde_json::from_value(Value::Object(map))
        .map_err(|error| ApiError::validation(format!("invalid step {} config: {error}", step.id)))
}

fn order_by_sql(order_by: &[OrderBy]) -> Result<String, ApiError> {
    if order_by.is_empty() {
        return Ok(String::new());
    }
    let parts = order_by
        .iter()
        .map(|order| {
            Ok(format!(
                "{} {}",
                sql_ident(&order.column)?,
                sql_direction(order.direction.as_deref())?
            ))
        })
        .collect::<Result<Vec<_>, ApiError>>()?;

    Ok(format!(" order by {}", parts.join(", ")))
}

fn brief_order_by_sql(order_by: &BriefOrderBy) -> Result<String, ApiError> {
    Ok(format!(
        " order by {} {}",
        sql_ident(&order_by.field)?,
        sql_direction(order_by.direction.as_deref())?
    ))
}

fn sql_ident(value: &str) -> Result<String, ApiError> {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        Ok(value.to_owned())
    } else {
        Err(ApiError::validation(format!(
            "unsupported SQL identifier: {value}"
        )))
    }
}

fn sql_direction(direction: Option<&str>) -> Result<&'static str, ApiError> {
    match direction.unwrap_or("asc").to_ascii_lowercase().as_str() {
        "asc" => Ok("asc"),
        "desc" => Ok("desc"),
        other => Err(ApiError::validation(format!(
            "unsupported order direction: {other}"
        ))),
    }
}

fn sql_literal_from_rendered(value: &str) -> String {
    if value.eq_ignore_ascii_case("null")
        || value.parse::<i64>().is_ok()
        || value.parse::<f64>().is_ok()
        || matches!(value, "true" | "false")
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "''"))
    }
}

fn value_as_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
}

fn value_as_str(value: &Value) -> Option<&str> {
    match value {
        Value::String(value) => Some(value),
        _ => None,
    }
}

fn value_equals_rendered(value: &Value, expected: &str) -> bool {
    if let Some(actual) = value_as_i64(value) {
        if let Ok(expected) = expected.parse::<i64>() {
            return actual == expected;
        }
    }
    if let Some(actual) = value.as_bool() {
        if let Ok(expected) = expected.parse::<bool>() {
            return actual == expected;
        }
    }

    value_as_str(value).is_some_and(|actual| actual == expected)
}

fn compare_values(left: Option<&Value>, right: Option<&Value>) -> Ordering {
    match (
        left.and_then(value_as_i64),
        right.and_then(value_as_i64),
        left.and_then(value_as_str),
        right.and_then(value_as_str),
    ) {
        (Some(left), Some(right), _, _) => left.cmp(&right),
        (_, _, Some(left), Some(right)) => left.cmp(right),
        _ => Ordering::Equal,
    }
}

#[derive(Clone, Debug)]
struct QueryColumn {
    sql: String,
}

#[derive(Debug, Deserialize)]
struct BranchConfig {
    when: BranchWhen,
    #[serde(rename = "then")]
    then_steps: Vec<FlowStep>,
    #[serde(rename = "else")]
    else_steps: Vec<FlowStep>,
}

#[derive(Debug, Deserialize)]
struct BranchWhen {
    row_count: BranchRowCount,
}

#[derive(Debug, Deserialize)]
struct BranchRowCount {
    table: String,
    equals: usize,
}

#[derive(Debug, Deserialize)]
struct LoopConfig {
    max_iterations: LoopMaxIterations,
    #[serde(default)]
    accumulators: HashMap<String, LoopAccumulator>,
    #[serde(default)]
    body: Vec<FlowStep>,
}

#[derive(Debug, Deserialize)]
struct LoopMaxIterations {
    slot: String,
}

#[derive(Debug, Deserialize)]
struct LoopAccumulator {
    kind: String,
    append_from: String,
}
