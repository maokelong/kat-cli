use crate::json::batches_to_query_result;
use crate::logical_source::ParsedTraceSource;
use crate::{QueryRequest, QueryResult, TraceEngineError, TraceResult};
use arrow_array::{ArrayRef, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use datafusion::datasource::MemTable;
use datafusion::prelude::SessionContext;
use std::collections::BTreeMap;
use std::sync::Arc;
use trace_model::ParsedTrace;

pub fn register_parsed_trace(ctx: &SessionContext, parsed: &ParsedTrace) -> TraceResult<()> {
    for (name, batch) in parsed.tables.batches() {
        let provider = MemTable::try_new(batch.schema(), vec![vec![batch]]).map_err(|err| {
            TraceEngineError::Engine(format!("failed to build MemTable {name}: {err}"))
        })?;
        ctx.register_table(name, Arc::new(provider))
            .map_err(|err| {
                TraceEngineError::Engine(format!("failed to register table {name}: {err}"))
            })?;
    }
    Ok(())
}

pub fn register_parsed_traces(
    ctx: &SessionContext,
    parsed_traces: &[ParsedTrace],
) -> TraceResult<()> {
    let sources = parsed_traces
        .iter()
        .enumerate()
        .map(|(index, parsed)| ParsedTraceSource {
            dataset_id: "dataset:anonymous".to_string(),
            source_id: format!("source_{index}"),
            trace_id: parsed.trace_id.clone(),
            parsed: parsed.clone(),
        })
        .collect();
    register_parsed_trace_sources(ctx, sources)
}

pub fn register_parsed_trace_sources(
    ctx: &SessionContext,
    sources: Vec<ParsedTraceSource>,
) -> TraceResult<()> {
    let mut batches_by_name: BTreeMap<String, Vec<RecordBatch>> = BTreeMap::new();

    for source in sources {
        for (name, batch) in source.parsed.tables.batches() {
            let enriched = append_provenance_columns(
                batch,
                &source.dataset_id,
                &source.source_id,
                &source.trace_id,
            )?;
            batches_by_name
                .entry(name.to_string())
                .or_default()
                .push(enriched);
        }
    }

    for (name, batches) in batches_by_name {
        if batches.is_empty() {
            continue;
        }
        let schema = batches[0].schema();
        let partitions = batches.into_iter().map(|batch| vec![batch]).collect();
        let provider = MemTable::try_new(schema, partitions).map_err(|err| {
            TraceEngineError::Engine(format!("failed to build merged MemTable {name}: {err}"))
        })?;
        ctx.register_table(name.as_str(), Arc::new(provider))
            .map_err(|err| {
                TraceEngineError::Engine(format!("failed to register merged table {name}: {err}"))
            })?;
    }

    Ok(())
}

fn append_provenance_columns(
    batch: RecordBatch,
    dataset_id: &str,
    source_id: &str,
    trace_id: &str,
) -> TraceResult<RecordBatch> {
    let row_count = batch.num_rows();
    let mut fields = batch.schema().fields().iter().cloned().collect::<Vec<_>>();
    let mut columns = batch.columns().to_vec();

    append_utf8_column_if_missing(
        &mut fields,
        &mut columns,
        "dataset_id",
        dataset_id,
        row_count,
    );
    append_utf8_column_if_missing(&mut fields, &mut columns, "source_id", source_id, row_count);
    append_utf8_column_if_missing(&mut fields, &mut columns, "trace_id", trace_id, row_count);

    RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).map_err(|err| {
        TraceEngineError::Engine(format!("failed to append provenance columns: {err}"))
    })
}

fn append_utf8_column_if_missing(
    fields: &mut Vec<Arc<Field>>,
    columns: &mut Vec<ArrayRef>,
    name: &str,
    value: &str,
    row_count: usize,
) {
    if fields.iter().any(|field| field.name() == name) {
        return;
    }

    fields.push(Arc::new(Field::new(name, DataType::Utf8, false)));
    columns.push(Arc::new(StringArray::from(vec![value; row_count])) as ArrayRef);
}

pub async fn query_parsed_trace(
    parsed: &ParsedTrace,
    request: QueryRequest,
) -> TraceResult<QueryResult> {
    let ctx = SessionContext::new();
    register_parsed_trace(&ctx, parsed)?;
    let dataframe = ctx
        .sql(&request.sql)
        .await
        .map_err(|err| TraceEngineError::UnsupportedSql(err.to_string()))?;
    let batches = dataframe
        .collect()
        .await
        .map_err(|err| TraceEngineError::Engine(format!("query execution failed: {err}")))?;
    batches_to_query_result(&batches, request.max_inline_rows)
}

pub async fn query_parsed_traces(
    parsed_traces: &[ParsedTrace],
    request: QueryRequest,
) -> TraceResult<QueryResult> {
    let ctx = SessionContext::new();
    register_parsed_traces(&ctx, parsed_traces)?;

    let dataframe = ctx
        .sql(&request.sql)
        .await
        .map_err(|err| TraceEngineError::UnsupportedSql(err.to_string()))?;
    let batches = dataframe
        .collect()
        .await
        .map_err(|err| TraceEngineError::Engine(format!("query execution failed: {err}")))?;
    batches_to_query_result(&batches, request.max_inline_rows)
}
