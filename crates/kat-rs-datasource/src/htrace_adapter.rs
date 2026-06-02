use crate::{
    build_dataset_cache_key, dataset_cache_manifest_path, write_dataset_cache_manifest,
    ArtifactStore, ColumnInspection, DatasetHandle, DatasetInput, DatasetInspection, DatasetState,
    DatasetSummary, DatasourceError, DatasourceQueryRequest, DatasourceResult, PhaseMetrics,
    QueryColumn, QueryEnvelope, QueryMetrics, QueryOutputMode, QueryStats, QueryStatus,
    SourceHandle, TableAvailability, TableCapability, TraceDatasource, TraceSource,
    PHASE_ARTIFACT_WRITE, PHASE_OPEN_DATASET, PHASE_PARSE_SOURCE, PHASE_QUERY_EXECUTE,
    PHASE_RESULT_SERIALIZE, PHASE_SESSION_BUILD, PHASE_SESSION_LOOKUP,
};
use async_trait::async_trait;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use trace_model::ParsedTrace;
use trace_parser::options::PARSE_PHASE_FILE_READ;
use trace_parser::{
    parse_trace_file_with_options, BytraceParser, HarmonyTraceParser, HiSysEventParser,
    HilogParser, HtraceParser, ParseOptions, PerfParser, RawTraceParser,
};
use trace_query::{ParsedTraceQuerySession, ParsedTraceSource};
use trace_query::{QueryRequest, SCHEMA_VERSION};

pub struct HtraceDatasource {
    datasets: Mutex<HashMap<String, Arc<DatasetState>>>,
}

impl HtraceDatasource {
    pub fn new() -> Self {
        Self {
            datasets: Mutex::new(HashMap::new()),
        }
    }

    pub async fn open_dataset(&self, input: DatasetInput) -> DatasourceResult<DatasetHandle> {
        let open_started = Instant::now();
        let DatasetInput {
            sources: input_sources,
            cache_dir,
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
        let cache_metadata = if let Some(cache_dir) = cache_dir.as_ref() {
            let cache_key = build_dataset_cache_key(SCHEMA_VERSION, &input_sources)?;
            let manifest_path = dataset_cache_manifest_path(cache_dir, &cache_key)?;
            Some((cache_key, manifest_path.exists()))
        } else {
            None
        };

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
        if let (Some(cache_dir), Some((cache_key, _))) =
            (cache_dir.as_ref(), cache_metadata.as_ref())
        {
            write_dataset_cache_manifest(cache_dir, &dataset_id, cache_key)?;
        }
        let cache_hit = cache_metadata
            .as_ref()
            .map(|(_, cache_hit)| *cache_hit)
            .unwrap_or(false);

        self.datasets
            .lock()
            .map_err(|_| DatasourceError::Engine("dataset cache lock poisoned".to_string()))?
            .insert(
                dataset_id,
                Arc::new(DatasetState::new(
                    handle.clone(),
                    parsed_sources,
                    cache_dir,
                    cache_hit,
                    open_phase_elapsed_ms,
                )),
            );

        Ok(handle)
    }

    pub async fn list_datasets(&self) -> DatasourceResult<Vec<DatasetSummary>> {
        let datasets = self
            .datasets
            .lock()
            .map_err(|_| DatasourceError::Engine("dataset cache lock poisoned".to_string()))?;
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
            .map_err(|_| DatasourceError::Engine("dataset cache lock poisoned".to_string()))?
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
            for (name, batch) in source.parsed.tables.batches() {
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
        let output = request.output.clone();
        let limits = request.limits.clone();
        let query_tag = request.query_tag.clone();
        let engine_max_inline_rows = match output {
            QueryOutputMode::InlineJson => limits.max_rows_inline,
            QueryOutputMode::Artifact => usize::MAX,
        };

        let query_started = Instant::now();
        let result = session
            .query(QueryRequest {
                sql: request.sql,
                max_inline_rows: engine_max_inline_rows,
            })
            .await?;
        phases.record(PHASE_QUERY_EXECUTE, query_started.elapsed());

        let serialize_started = Instant::now();
        let mut rows = result.rows;
        let mut artifacts = Vec::new();
        let mut diagnostics = Vec::new();
        let mut bytes_inline = serde_json::to_vec(&rows)
            .map(|bytes| bytes.len())
            .unwrap_or_default();
        phases.record(PHASE_RESULT_SERIALIZE, serialize_started.elapsed());
        let mut status = match result.status.as_str() {
            "ok" => QueryStatus::Ok,
            "empty_result" => QueryStatus::EmptyResult,
            _ => QueryStatus::EngineError,
        };

        match output {
            QueryOutputMode::InlineJson
                if status == QueryStatus::Ok && bytes_inline > limits.max_result_bytes_inline =>
            {
                status = QueryStatus::ResultTooLarge;
                diagnostics.push(format!(
                    "inline result size {bytes_inline} bytes exceeds max_result_bytes_inline {}",
                    limits.max_result_bytes_inline
                ));
                rows.clear();
                bytes_inline = 0;
            }
            QueryOutputMode::InlineJson => {}
            QueryOutputMode::Artifact if status == QueryStatus::Ok => {
                let artifact_started = Instant::now();
                let artifact_root = state
                    .cache_dir
                    .clone()
                    .unwrap_or_else(|| std::env::temp_dir().join("kat-rs-datasource"))
                    .join("artifacts");
                let artifact = ArtifactStore::new(artifact_root)?.write_jsonl(
                    &handle.dataset_id,
                    query_tag.as_deref(),
                    &rows,
                )?;
                phases.record(PHASE_ARTIFACT_WRITE, artifact_started.elapsed());
                artifacts.push(artifact);
                rows.clear();
                bytes_inline = 0;
            }
            QueryOutputMode::Artifact => {}
        }

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
            artifacts,
            stats: QueryStats {
                rows_returned: result.stats.rows_returned,
                bytes_inline,
                truncated: result.stats.truncated,
            },
            metrics: QueryMetrics {
                elapsed_ms: started.elapsed().as_millis() as u64,
                cache_hit: state.cache_hit,
                phase_elapsed_ms: phases.into_inner(),
                rows_returned: result.stats.rows_returned,
                bytes_inline,
            },
            diagnostics,
        })
    }

    fn dataset_state(&self, handle: &DatasetHandle) -> DatasourceResult<Arc<DatasetState>> {
        self.datasets
            .lock()
            .map_err(|_| DatasourceError::Engine("dataset cache lock poisoned".to_string()))?
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
    let Some(format_hint) = source.format_hint.as_deref() else {
        let outcome = parse_trace_file_with_options(&source.path, &options)?;
        return Ok(DatasourceParseOutcome {
            parsed: outcome.parsed,
            phase_elapsed_ms: outcome.phase_elapsed_ms,
        });
    };

    let parsed = match format_hint.to_ascii_lowercase().as_str() {
        "htrace" => parse_with_parser(HtraceParser::default(), &source.path)?,
        "bytrace" | "bytrace-text" | "bytrace_text" => {
            let read_started = Instant::now();
            let bytes = std::fs::read(&source.path)?;
            let file_read_elapsed_ms = read_started.elapsed().as_millis() as u64;
            let mut parser = BytraceParser::default();
            let outcome = parser.parse_bytes_with_options(&bytes, &options)?;
            let mut phase_elapsed_ms = outcome.phase_elapsed_ms;
            *phase_elapsed_ms
                .entry(PARSE_PHASE_FILE_READ.to_string())
                .or_default() += file_read_elapsed_ms;
            return Ok(DatasourceParseOutcome {
                parsed: outcome.parsed,
                phase_elapsed_ms,
            });
        }
        "rawtrace" | "raw-trace" | "raw_trace" => {
            parse_with_parser(RawTraceParser::default(), &source.path)?
        }
        "hisysevent" | "hisysevent-text" | "hisysevent_text" => {
            parse_with_parser(HiSysEventParser::default(), &source.path)?
        }
        "hilog" | "hilog-text" | "hilog_text" => {
            parse_with_parser(HilogParser::default(), &source.path)?
        }
        "perf" => parse_with_parser(PerfParser::default(), &source.path)?,
        other => Err(DatasourceError::InvalidInput(format!(
            "unsupported trace format_hint {other}"
        )))?,
    };
    Ok(DatasourceParseOutcome {
        parsed,
        phase_elapsed_ms: BTreeMap::new(),
    })
}

fn parse_with_parser<P>(mut parser: P, path: &Path) -> DatasourceResult<ParsedTrace>
where
    P: HarmonyTraceParser,
{
    Ok(parser.parse_file(path)?)
}

fn accumulate_parse_phases(target: &mut BTreeMap<String, u64>, phases: &BTreeMap<String, u64>) {
    for (phase, elapsed_ms) in phases {
        *target.entry(phase.clone()).or_default() += *elapsed_ms;
    }
}

impl Default for HtraceDatasource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TraceDatasource for HtraceDatasource {
    async fn open_dataset(&self, input: DatasetInput) -> DatasourceResult<DatasetHandle> {
        HtraceDatasource::open_dataset(self, input).await
    }

    async fn list_datasets(&self) -> DatasourceResult<Vec<DatasetSummary>> {
        HtraceDatasource::list_datasets(self).await
    }

    async fn close_dataset(&self, handle: &DatasetHandle) -> DatasourceResult<()> {
        HtraceDatasource::close_dataset(self, handle).await
    }

    async fn inspect(&self, handle: &DatasetHandle) -> DatasourceResult<DatasetInspection> {
        HtraceDatasource::inspect(self, handle).await
    }

    async fn query(
        &self,
        handle: &DatasetHandle,
        request: DatasourceQueryRequest,
    ) -> DatasourceResult<QueryEnvelope> {
        HtraceDatasource::query(self, handle, request).await
    }
}
