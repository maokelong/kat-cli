use crate::{
    ColumnInspection, DatasetHandle, DatasetInput, DatasetInspection, DatasetState, DatasetSummary,
    DatasourceError, DatasourceQueryRequest, DatasourceResult, PhaseMetrics, QueryColumn,
    QueryEnvelope, QueryMetrics, QueryStats, QueryStatus, SourceHandle, TableAvailability,
    TableCapability, TraceDatasource, TraceSource, PHASE_OPEN_DATASET, PHASE_PARSE_SOURCE,
    PHASE_QUERY_EXECUTE, PHASE_RESULT_SERIALIZE, PHASE_SESSION_BUILD, PHASE_SESSION_LOOKUP,
};
use async_trait::async_trait;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use trace_model::ParsedTrace;
use trace_parser::{parse_trace_file_with_options, ParseOptions};
use trace_query::{ParsedTraceQuerySession, ParsedTraceSource};
use trace_query::{QueryRequest, SCHEMA_VERSION};

pub struct TraceDatasourceLib {
    datasets: Mutex<HashMap<String, Arc<DatasetState>>>,
}

impl TraceDatasourceLib {
    pub fn new() -> Self {
        Self {
            datasets: Mutex::new(HashMap::new()),
        }
    }

    pub async fn open_dataset(&self, input: DatasetInput) -> DatasourceResult<DatasetHandle> {
        let open_started = Instant::now();
        let DatasetInput {
            sources: input_sources,
            required_tables,
        } = input;
        if input_sources.is_empty() {
            return Err(DatasourceError::InvalidInput(
                "dataset must contain at least one source".to_string(),
            ));
        }
        let mut parsed_sources = Vec::with_capacity(input_sources.len());
        let mut sources = Vec::with_capacity(input_sources.len());
        let mut parse_elapsed_ms = 0_u64;
        let mut open_phase_elapsed_ms = BTreeMap::new();

        for (index, source) in input_sources.into_iter().enumerate() {
            let parse_started = Instant::now();
            let parse_outcome = parse_trace_source(&source, &required_tables)?;
            let parsed = parse_outcome.parsed;
            accumulate_parse_phases(&mut open_phase_elapsed_ms, &parse_outcome.phase_elapsed_ms);
            parse_elapsed_ms += parse_started.elapsed().as_millis() as u64;
            let source_id = source
                .source_name
                .unwrap_or_else(|| format!("source_{index}"));
            let trace_id = parsed.trace_id.clone();
            sources.push(SourceHandle {
                source_id: source_id.clone(),
                trace_id: trace_id.clone(),
                path: source.path,
            });
            parsed_sources.push((source_id, trace_id, parsed));
        }

        let dataset_id = format!(
            "dataset:{}",
            sources
                .iter()
                .map(|source| source.trace_id.as_str())
                .collect::<Vec<_>>()
                .join("+")
        );

        let handle = DatasetHandle {
            dataset_id: dataset_id.clone(),
            sources,
        };
        let parsed_sources = parsed_sources
            .into_iter()
            .map(|(source_id, trace_id, parsed)| ParsedTraceSource {
                dataset_id: dataset_id.clone(),
                source_id,
                trace_id,
                parsed,
            })
            .collect();
        open_phase_elapsed_ms.insert(PHASE_PARSE_SOURCE.to_string(), parse_elapsed_ms);
        open_phase_elapsed_ms.insert(
            PHASE_OPEN_DATASET.to_string(),
            open_started.elapsed().as_millis() as u64,
        );

        self.datasets
            .lock()
            .map_err(|_| DatasourceError::Engine("dataset registry lock poisoned".to_string()))?
            .insert(
                dataset_id,
                Arc::new(DatasetState::new(
                    handle.clone(),
                    parsed_sources,
                    open_phase_elapsed_ms,
                )),
            );

        Ok(handle)
    }

    pub async fn list_datasets(&self) -> DatasourceResult<Vec<DatasetSummary>> {
        let datasets = self
            .datasets
            .lock()
            .map_err(|_| DatasourceError::Engine("dataset registry lock poisoned".to_string()))?;
        let mut summaries = datasets
            .values()
            .map(|state| DatasetSummary {
                dataset_id: state.handle.dataset_id.clone(),
                source_count: state.handle.sources.len(),
                source_ids: state
                    .handle
                    .sources
                    .iter()
                    .map(|source| source.source_id.clone())
                    .collect(),
            })
            .collect::<Vec<_>>();
        summaries.sort_by(|left, right| left.dataset_id.cmp(&right.dataset_id));
        Ok(summaries)
    }

    pub async fn close_dataset(&self, handle: &DatasetHandle) -> DatasourceResult<()> {
        let removed = self
            .datasets
            .lock()
            .map_err(|_| DatasourceError::Engine("dataset registry lock poisoned".to_string()))?
            .remove(&handle.dataset_id);

        if removed.is_none() {
            return Err(DatasourceError::InvalidInput(format!(
                "unknown dataset handle {}",
                handle.dataset_id
            )));
        }

        Ok(())
    }

    pub async fn inspect(&self, handle: &DatasetHandle) -> DatasourceResult<DatasetInspection> {
        let state = self.dataset_state(handle)?;
        let mut tables = BTreeMap::<String, TableCapability>::new();

        for source in state.sources.iter() {
            for (name, batch) in source.parsed.batches() {
                let columns = batch
                    .schema()
                    .fields()
                    .iter()
                    .map(|field| ColumnInspection {
                        name: field.name().clone(),
                        data_type: field.data_type().to_string(),
                        unit: None,
                        nullable: Some(field.is_nullable()),
                    })
                    .collect();
                let entry = tables
                    .entry(name.to_string())
                    .or_insert_with(|| TableCapability {
                        available: true,
                        availability: TableAvailability::Available,
                        row_count: 0,
                        reason: None,
                        columns,
                    });
                entry.row_count += batch.num_rows();
            }
        }
        for capability in tables.values_mut() {
            if capability.row_count == 0 {
                capability.availability = TableAvailability::Empty;
            }
        }

        Ok(DatasetInspection {
            schema_version: SCHEMA_VERSION.to_string(),
            dataset_id: handle.dataset_id.clone(),
            source_count: handle.sources.len(),
            tables,
        })
    }

    pub async fn query(
        &self,
        handle: &DatasetHandle,
        request: DatasourceQueryRequest,
    ) -> DatasourceResult<QueryEnvelope> {
        let state = self.dataset_state(handle)?;
        let started = Instant::now();
        let mut phases = PhaseMetrics::default();
        phases.extend(&state.open_phase_elapsed_ms);
        let session = self.query_session(&state, &mut phases)?;

        let query_started = Instant::now();
        let result = session
            .query(QueryRequest {
                sql: request.sql,
                max_inline_rows: usize::MAX,
            })
            .await?;
        phases.record(PHASE_QUERY_EXECUTE, query_started.elapsed());

        let serialize_started = Instant::now();
        let rows = result.rows;
        let bytes_inline = serde_json::to_vec(&rows)
            .map(|bytes| bytes.len())
            .unwrap_or_default();
        phases.record(PHASE_RESULT_SERIALIZE, serialize_started.elapsed());
        let status = match result.status.as_str() {
            "ok" => QueryStatus::Ok,
            "empty_result" => QueryStatus::EmptyResult,
            _ => QueryStatus::EngineError,
        };

        Ok(QueryEnvelope {
            status,
            schema_version: SCHEMA_VERSION.to_string(),
            dataset_id: handle.dataset_id.clone(),
            columns: result
                .columns
                .into_iter()
                .map(|column| QueryColumn {
                    name: column.name,
                    data_type: column.data_type,
                    unit: None,
                    nullable: None,
                })
                .collect(),
            rows,
            stats: QueryStats {
                rows_returned: result.stats.rows_returned,
                bytes_inline,
                truncated: result.stats.truncated,
            },
            metrics: QueryMetrics {
                elapsed_ms: started.elapsed().as_millis() as u64,
                phase_elapsed_ms: phases.into_inner(),
                rows_returned: result.stats.rows_returned,
                bytes_inline,
            },
            diagnostics: Vec::new(),
        })
    }

    fn dataset_state(&self, handle: &DatasetHandle) -> DatasourceResult<Arc<DatasetState>> {
        self.datasets
            .lock()
            .map_err(|_| DatasourceError::Engine("dataset registry lock poisoned".to_string()))?
            .get(&handle.dataset_id)
            .cloned()
            .ok_or_else(|| {
                DatasourceError::InvalidInput(format!(
                    "unknown dataset handle {}",
                    handle.dataset_id
                ))
            })
    }

    fn query_session(
        &self,
        state: &Arc<DatasetState>,
        phases: &mut PhaseMetrics,
    ) -> DatasourceResult<Arc<ParsedTraceQuerySession>> {
        let lookup_started = Instant::now();
        let mut guard = state
            .query_session
            .lock()
            .map_err(|_| DatasourceError::Engine("query session lock poisoned".to_string()))?;
        if let Some(session) = guard.as_ref() {
            phases.record(PHASE_SESSION_LOOKUP, lookup_started.elapsed());
            return Ok(Arc::clone(session));
        }
        phases.record(PHASE_SESSION_LOOKUP, lookup_started.elapsed());

        let build_started = Instant::now();
        let session = Arc::new(ParsedTraceQuerySession::from_parsed_trace_sources(
            state.sources.as_ref().clone(),
        )?);
        phases.record(PHASE_SESSION_BUILD, build_started.elapsed());
        *guard = Some(Arc::clone(&session));
        Ok(session)
    }
}

struct DatasourceParseOutcome {
    parsed: ParsedTrace,
    phase_elapsed_ms: BTreeMap<String, u64>,
}

fn parse_trace_source(
    source: &TraceSource,
    required_tables: &[String],
) -> DatasourceResult<DatasourceParseOutcome> {
    let options = ParseOptions::for_required_tables(required_tables.iter().map(String::as_str));
    if let Some(format_hint) = source.format_hint.as_deref() {
        if format_hint.eq_ignore_ascii_case("htrace") {
            let outcome = parse_trace_file_with_options(&source.path, &options)?;
            return Ok(DatasourceParseOutcome {
                parsed: outcome.parsed,
                phase_elapsed_ms: outcome.phase_elapsed_ms,
            });
        }
        return Err(DatasourceError::InvalidInput(format!(
            "unsupported trace format_hint {format_hint}"
        )));
    }

    let outcome = parse_trace_file_with_options(&source.path, &options)?;
    Ok(DatasourceParseOutcome {
        parsed: outcome.parsed,
        phase_elapsed_ms: outcome.phase_elapsed_ms,
    })
}

fn accumulate_parse_phases(target: &mut BTreeMap<String, u64>, phases: &BTreeMap<String, u64>) {
    for (phase, elapsed_ms) in phases {
        *target.entry(phase.clone()).or_default() += *elapsed_ms;
    }
}

impl Default for TraceDatasourceLib {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TraceDatasource for TraceDatasourceLib {
    async fn open_dataset(&self, input: DatasetInput) -> DatasourceResult<DatasetHandle> {
        TraceDatasourceLib::open_dataset(self, input).await
    }

    async fn list_datasets(&self) -> DatasourceResult<Vec<DatasetSummary>> {
        TraceDatasourceLib::list_datasets(self).await
    }

    async fn close_dataset(&self, handle: &DatasetHandle) -> DatasourceResult<()> {
        TraceDatasourceLib::close_dataset(self, handle).await
    }

    async fn inspect(&self, handle: &DatasetHandle) -> DatasourceResult<DatasetInspection> {
        TraceDatasourceLib::inspect(self, handle).await
    }

    async fn query(
        &self,
        handle: &DatasetHandle,
        request: DatasourceQueryRequest,
    ) -> DatasourceResult<QueryEnvelope> {
        TraceDatasourceLib::query(self, handle, request).await
    }
}
