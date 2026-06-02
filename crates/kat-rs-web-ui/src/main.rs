use anyhow::{Context, Result};
use axum::{
    extract::{DefaultBodyLimit, Multipart, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use kat_rs_datasource::{
    inspect_dataset_for_ui, DatasetHandle, DatasetInput, DatasetUiInspection,
    DatasourceQueryRequest, DatasourceService, HtraceDatasource, QueryEnvelope, TraceSource,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_UPLOAD_BYTES: usize = 1024 * 1024 * 1024;

#[derive(Debug, Parser)]
#[command(name = "kat-rs-web-ui")]
#[command(about = "Small local web UI for querying kat-rs datasource data")]
struct Cli {
    #[arg(long = "trace")]
    traces: Vec<PathBuf>,
    #[arg(long, default_value = "127.0.0.1:8787")]
    listen: SocketAddr,
    #[arg(long, default_value = "tests/fixtures/traces")]
    fixture_dir: PathBuf,
    #[arg(long)]
    upload_dir: Option<PathBuf>,
}

#[derive(Clone)]
struct AppState {
    datasource: Arc<DatasourceService<HtraceDatasource>>,
    datasets: Arc<Mutex<DatasetRegistry>>,
    fixture_dir: PathBuf,
    upload_dir: PathBuf,
}

#[derive(Default)]
struct DatasetRegistry {
    active_dataset_id: Option<String>,
    datasets: HashMap<String, WebDataset>,
}

#[derive(Clone)]
struct WebDataset {
    label: String,
    kind: DatasetKind,
    trace_paths: Vec<PathBuf>,
    handle: DatasetHandle,
    inspection: DatasetUiInspection,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum DatasetKind {
    Current,
    Fixture,
    Upload,
}

#[derive(Debug, Deserialize)]
struct QueryPayload {
    dataset_id: Option<String>,
    sql: String,
    #[serde(default = "default_max_inline_rows")]
    max_inline_rows: usize,
}

#[derive(Debug, Deserialize)]
struct DatasetQuery {
    dataset_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenFixturePayload {
    path: String,
}

#[derive(Debug, Serialize)]
struct DatasetsPayload {
    active_dataset_id: Option<String>,
    datasets: Vec<DatasetSummaryPayload>,
    fixtures: Vec<FixturePayload>,
}

#[derive(Debug, Serialize)]
struct DatasetSummaryPayload {
    dataset_id: String,
    label: String,
    kind: DatasetKind,
    paths: Vec<PathBuf>,
    source_count: usize,
    table_count: usize,
}

#[derive(Debug, Serialize)]
struct FixturePayload {
    name: String,
    path: String,
    size_bytes: u64,
}

#[derive(Debug, Serialize)]
struct OpenDatasetPayload {
    dataset_id: String,
    label: String,
}

#[derive(Debug, Serialize)]
struct ErrorPayload {
    error: String,
}

fn default_max_inline_rows() -> usize {
    1_000
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let trace_paths = cli.traces;
    let trace_display = trace_paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let upload_dir = cli
        .upload_dir
        .unwrap_or_else(|| std::env::temp_dir().join("kat-rs-web-ui").join("uploads"));
    let state = build_state(trace_paths, cli.fixture_dir, upload_dir)
        .await
        .with_context(|| {
            if trace_display.is_empty() {
                "failed to initialize kat-rs-web-ui".to_string()
            } else {
                format!("failed to open datasource for {trace_display}")
            }
        })?;

    let app = Router::new()
        .route("/", get(index))
        .route("/api/datasets", get(datasets))
        .route("/api/datasets/fixture", post(open_fixture))
        .route("/api/datasets/upload", post(upload_trace))
        .route("/api/inspect", get(inspect))
        .route("/api/query", post(query))
        .with_state(state)
        .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES));

    let listener = tokio::net::TcpListener::bind(cli.listen).await?;
    println!("kat-rs-web-ui listening on http://{}", cli.listen);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn build_state(
    trace_paths: Vec<PathBuf>,
    fixture_dir: PathBuf,
    upload_dir: PathBuf,
) -> Result<Arc<AppState>> {
    let datasource = Arc::new(DatasourceService::new(HtraceDatasource::new()));
    let state = Arc::new(AppState {
        datasource,
        datasets: Arc::new(Mutex::new(DatasetRegistry::default())),
        fixture_dir,
        upload_dir,
    });

    if !trace_paths.is_empty() {
        let dataset = open_web_dataset(
            &state.datasource,
            trace_paths,
            "Current datasource".to_string(),
            DatasetKind::Current,
        )
        .await?;
        insert_dataset(&state, dataset, true).map_err(|err| anyhow::anyhow!(err.message))?;
    }

    Ok(state)
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn datasets(State(state): State<Arc<AppState>>) -> Result<Json<DatasetsPayload>, ApiError> {
    let fixtures = list_fixtures(&state.fixture_dir)?;
    let registry = state
        .datasets
        .lock()
        .map_err(|_| ApiError::internal("dataset registry lock poisoned"))?;
    let mut datasets = registry
        .datasets
        .iter()
        .map(|(dataset_id, dataset)| DatasetSummaryPayload {
            dataset_id: dataset_id.clone(),
            label: dataset.label.clone(),
            kind: dataset.kind.clone(),
            paths: dataset.trace_paths.clone(),
            source_count: dataset.inspection.source_count,
            table_count: dataset.inspection.tables.len(),
        })
        .collect::<Vec<_>>();
    datasets.sort_by(|left, right| left.label.cmp(&right.label));

    Ok(Json(DatasetsPayload {
        active_dataset_id: registry.active_dataset_id.clone(),
        datasets,
        fixtures,
    }))
}

async fn open_fixture(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<OpenFixturePayload>,
) -> Result<Json<OpenDatasetPayload>, ApiError> {
    let fixture_path = resolve_fixture_path(&state.fixture_dir, &payload.path)?;
    let label = format!("Fixture {}", payload.path.replace('\\', "/"));
    let dataset = open_web_dataset(
        &state.datasource,
        vec![fixture_path],
        label.clone(),
        DatasetKind::Fixture,
    )
    .await
    .map_err(|err| ApiError::bad_request(err.to_string()))?;
    let dataset_id = insert_dataset(&state, dataset, true)?;
    Ok(Json(OpenDatasetPayload { dataset_id, label }))
}

async fn upload_trace(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<OpenDatasetPayload>, ApiError> {
    fs::create_dir_all(&state.upload_dir)
        .map_err(|err| ApiError::internal(format!("failed to create upload dir: {err}")))?;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| ApiError::bad_request(format!("invalid multipart upload: {err}")))?
    {
        if field.name() != Some("trace") {
            continue;
        }

        let original_name = field.file_name().unwrap_or("uploaded.trace").to_string();
        let safe_name = safe_upload_name(&original_name);
        let bytes = field
            .bytes()
            .await
            .map_err(|err| ApiError::bad_request(format!("failed to read upload: {err}")))?;
        if bytes.is_empty() {
            return Err(ApiError::bad_request("uploaded trace file is empty"));
        }

        let saved_path = state
            .upload_dir
            .join(format!("{}-{safe_name}", unix_nanos()));
        fs::write(&saved_path, bytes)
            .map_err(|err| ApiError::internal(format!("failed to save upload: {err}")))?;

        let label = format!("Upload {original_name}");
        let dataset = open_web_dataset(
            &state.datasource,
            vec![saved_path],
            label.clone(),
            DatasetKind::Upload,
        )
        .await
        .map_err(|err| ApiError::bad_request(err.to_string()))?;
        let dataset_id = insert_dataset(&state, dataset, true)?;
        return Ok(Json(OpenDatasetPayload { dataset_id, label }));
    }

    Err(ApiError::bad_request(
        "multipart request must include a trace file field named trace",
    ))
}

async fn inspect(
    State(state): State<Arc<AppState>>,
    Query(query): Query<DatasetQuery>,
) -> Result<Json<Value>, ApiError> {
    let (dataset_id, dataset) = selected_dataset(&state, query.dataset_id.as_deref())?;
    Ok(Json(json!({
        "trace": {
            "path": dataset.trace_paths.first().cloned(),
            "paths": dataset.trace_paths,
            "dataset_id": dataset_id,
            "datasource_dataset_id": dataset.handle.dataset_id,
            "label": dataset.label,
            "kind": dataset.kind,
            "trace_id": dataset.inspection.trace.trace_id,
            "start_ts": dataset.inspection.trace.start_ts,
            "end_ts": dataset.inspection.trace.end_ts,
            "clock_domain": dataset.inspection.trace.clock_domain,
            "sources": dataset.inspection.trace.sources
        },
        "tables": dataset
            .inspection
            .tables
            .iter()
            .map(|(name, table)| {
                (
                    name.clone(),
                    json!({
                        "rows": table.row_count,
                        "columns": table
                            .columns
                            .iter()
                            .map(|column| json!({
                                "name": column.name,
                                "type": column.data_type
                            }))
                            .collect::<Vec<_>>()
                    }),
                )
            })
            .collect::<serde_json::Map<_, _>>()
    })))
}

async fn query(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<QueryPayload>,
) -> Result<Json<QueryEnvelope>, ApiError> {
    if payload.sql.trim().is_empty() {
        return Err(ApiError::bad_request("SQL is empty"));
    }

    let result = run_datasource_query(
        &state,
        payload.dataset_id.as_deref(),
        payload.sql,
        payload.max_inline_rows,
    )
    .await?;

    Ok(Json(result))
}

async fn run_datasource_query(
    state: &AppState,
    dataset_id: Option<&str>,
    sql: String,
    max_inline_rows: usize,
) -> Result<QueryEnvelope, ApiError> {
    let (_, dataset) = selected_dataset(state, dataset_id)?;
    let mut request = DatasourceQueryRequest::new(sql);
    request.limits.max_rows_inline = max_inline_rows;
    state
        .datasource
        .query(&dataset.handle, request)
        .await
        .map_err(|err| ApiError::bad_request(err.to_string()))
}

async fn open_web_dataset(
    datasource: &DatasourceService<HtraceDatasource>,
    trace_paths: Vec<PathBuf>,
    label: String,
    kind: DatasetKind,
) -> Result<WebDataset> {
    let handle = datasource
        .open_dataset(DatasetInput {
            sources: trace_paths
                .iter()
                .cloned()
                .map(|path| TraceSource {
                    path,
                    format_hint: None,
                    source_name: None,
                })
                .collect(),
            cache_dir: None,
            required_tables: Vec::new(),
        })
        .await?;
    let inspection = inspect_dataset_for_ui(datasource, &handle).await?;

    Ok(WebDataset {
        label,
        kind,
        trace_paths,
        handle,
        inspection,
    })
}

fn insert_dataset(
    state: &AppState,
    dataset: WebDataset,
    make_active: bool,
) -> Result<String, ApiError> {
    let base_dataset_id = dataset.handle.dataset_id.clone();
    let mut registry = state
        .datasets
        .lock()
        .map_err(|_| ApiError::internal("dataset registry lock poisoned"))?;
    let dataset_id = unique_dataset_id(&registry.datasets, &base_dataset_id);
    registry.datasets.insert(dataset_id.clone(), dataset);
    if make_active || registry.active_dataset_id.is_none() {
        registry.active_dataset_id = Some(dataset_id.clone());
    }
    Ok(dataset_id)
}

fn selected_dataset(
    state: &AppState,
    dataset_id: Option<&str>,
) -> Result<(String, WebDataset), ApiError> {
    let registry = state
        .datasets
        .lock()
        .map_err(|_| ApiError::internal("dataset registry lock poisoned"))?;
    let selected_id = dataset_id
        .map(ToString::to_string)
        .or_else(|| registry.active_dataset_id.clone())
        .ok_or_else(|| ApiError::bad_request("no dataset is open"))?;

    registry
        .datasets
        .get(&selected_id)
        .cloned()
        .map(|dataset| (selected_id.clone(), dataset))
        .ok_or_else(|| ApiError::bad_request(format!("unknown dataset_id {selected_id}")))
}

fn unique_dataset_id(datasets: &HashMap<String, WebDataset>, base_dataset_id: &str) -> String {
    if !datasets.contains_key(base_dataset_id) {
        return base_dataset_id.to_string();
    }

    let mut index = 2;
    loop {
        let candidate = format!("{base_dataset_id}#{index}");
        if !datasets.contains_key(&candidate) {
            return candidate;
        }
        index += 1;
    }
}

fn list_fixtures(fixture_dir: &Path) -> Result<Vec<FixturePayload>, ApiError> {
    if !fixture_dir.exists() {
        return Ok(Vec::new());
    }
    let root = fixture_dir
        .canonicalize()
        .map_err(|err| ApiError::internal(format!("failed to read fixture dir: {err}")))?;
    let mut fixtures = Vec::new();
    collect_fixture_files(&root, &root, &mut fixtures)?;
    fixtures.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(fixtures)
}

fn collect_fixture_files(
    root: &Path,
    dir: &Path,
    fixtures: &mut Vec<FixturePayload>,
) -> Result<(), ApiError> {
    for entry in fs::read_dir(dir)
        .map_err(|err| ApiError::internal(format!("failed to list fixtures: {err}")))?
    {
        let entry =
            entry.map_err(|err| ApiError::internal(format!("failed to read fixture: {err}")))?;
        let path = entry.path();
        if path.is_dir() {
            collect_fixture_files(root, &path, fixtures)?;
            continue;
        }
        if !is_trace_file(&path) {
            continue;
        }
        let metadata = entry
            .metadata()
            .map_err(|err| ApiError::internal(format!("failed to stat fixture: {err}")))?;
        let relative = path
            .strip_prefix(root)
            .map_err(|err| ApiError::internal(format!("failed to resolve fixture: {err}")))?
            .to_string_lossy()
            .replace('\\', "/");
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(relative.as_str())
            .to_string();
        fixtures.push(FixturePayload {
            name,
            path: relative,
            size_bytes: metadata.len(),
        });
    }
    Ok(())
}

fn resolve_fixture_path(fixture_dir: &Path, relative: &str) -> Result<PathBuf, ApiError> {
    if relative.trim().is_empty() {
        return Err(ApiError::bad_request("fixture path is empty"));
    }
    let root = fixture_dir
        .canonicalize()
        .map_err(|err| ApiError::internal(format!("failed to read fixture dir: {err}")))?;
    let candidate = root.join(relative.replace('\\', "/"));
    let resolved = candidate
        .canonicalize()
        .map_err(|err| ApiError::bad_request(format!("fixture not found: {err}")))?;
    if !resolved.starts_with(&root) {
        return Err(ApiError::bad_request(
            "fixture path must stay under the fixture directory",
        ));
    }
    if !resolved.is_file() || !is_trace_file(&resolved) {
        return Err(ApiError::bad_request("fixture path is not a trace file"));
    }
    Ok(resolved)
}

fn is_trace_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "htrace" | "bin" | "data" | "txt" | "systrace" | "zip"
            )
        })
        .unwrap_or(false)
}

fn safe_upload_name(name: &str) -> String {
    let sanitized = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.trim_matches('_').is_empty() {
        "uploaded.trace".to_string()
    } else {
        sanitized
    }
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorPayload {
                error: self.message,
            }),
        )
            .into_response()
    }
}

const INDEX_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>kat-rs Web UI</title>
  <style>
    :root {
      color-scheme: light;
      --bg: #f7f8fa;
      --panel: #ffffff;
      --text: #17202a;
      --muted: #64707d;
      --line: #d8dde4;
      --accent: #176b87;
      --accent-2: #7b4d8d;
      --danger: #a63d40;
      --ok: #237455;
      --code: #101820;
      --code-text: #eef3f7;
      --shadow: 0 10px 26px rgba(20, 28, 38, 0.08);
    }

    * {
      box-sizing: border-box;
    }

    body {
      margin: 0;
      min-height: 100vh;
      background: var(--bg);
      color: var(--text);
      font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      letter-spacing: 0;
    }

    .app {
      display: grid;
      grid-template-columns: clamp(320px, 24vw, 380px) minmax(0, 1fr);
      min-height: 100vh;
    }

    aside {
      display: grid;
      grid-template-rows: auto auto clamp(260px, 42vh, 420px) minmax(120px, 1fr);
      gap: 14px;
      max-height: 100vh;
      border-right: 1px solid var(--line);
      background: #f4f7f9;
      padding: 16px;
      overflow: hidden;
      min-width: 0;
    }

    main {
      display: grid;
      grid-template-rows: auto minmax(210px, 32vh) minmax(0, 1fr);
      min-width: 0;
      max-height: 100vh;
      overflow: hidden;
    }

    header {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 16px;
      padding: 16px 20px;
      border-bottom: 1px solid var(--line);
      background: var(--panel);
    }

    h1 {
      margin: 0;
      font-size: 18px;
      line-height: 1.2;
      font-weight: 700;
    }

    .trace-meta {
      color: var(--muted);
      font-size: 12px;
      line-height: 1.45;
      max-height: 54px;
      overflow: auto;
      overflow-wrap: anywhere;
      padding-right: 4px;
    }

    .sidebar-section {
      min-width: 0;
      min-height: 0;
      overflow: hidden;
      display: grid;
      border: 1px solid var(--line);
      border-radius: 8px;
      background: #ffffff;
      padding: 12px;
    }

    .section-head {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 10px;
      min-width: 0;
      padding-bottom: 10px;
      border-bottom: 1px solid #edf0f3;
    }

    .section-title {
      margin: 0;
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      color: var(--muted);
      font-size: 11px;
      font-weight: 800;
      line-height: 1.2;
      text-transform: uppercase;
    }

    .section-kicker {
      flex: 0 0 auto;
      color: var(--muted);
      font-size: 11px;
      font-weight: 650;
    }

    .sidebar-source {
      grid-template-rows: auto auto minmax(0, 1fr);
      gap: 10px;
    }

    .sidebar-brand {
      display: grid;
      gap: 5px;
      min-width: 0;
    }

    .sidebar-brand h1 {
      font-size: 19px;
    }

    .source-panel {
      display: grid;
      gap: 9px;
      min-width: 0;
      overflow: auto;
      padding-right: 2px;
    }

    .field {
      display: grid;
      gap: 5px;
      min-width: 0;
      color: var(--muted);
      font-size: 11px;
      font-weight: 700;
      text-transform: uppercase;
    }

    .field select,
    .field input[type="file"] {
      width: 100%;
      min-width: 0;
      min-height: 32px;
      border: 1px solid var(--line);
      border-radius: 6px;
      background: #ffffff;
      color: var(--text);
      font: inherit;
      font-size: 12px;
      padding: 5px 7px;
      text-transform: none;
      font-weight: 500;
      line-height: 1.3;
    }

    .source-actions {
      display: grid;
      grid-template-columns: 1fr;
      gap: 8px;
      min-width: 0;
    }

    .status {
      min-width: 120px;
      text-align: right;
      color: var(--muted);
      font-size: 12px;
    }

    .table-list {
      display: grid;
      gap: 10px;
      min-height: 0;
      overflow: auto;
      padding-right: 4px;
    }

    .table-browser {
      grid-template-rows: auto auto minmax(0, 1fr);
      gap: 10px;
      height: 100%;
    }

    .table-toolbar {
      display: grid;
      grid-template-columns: minmax(0, 1fr) auto;
      gap: 8px;
      align-items: center;
    }

    .table-search {
      min-height: 32px;
      width: 100%;
      border: 1px solid var(--line);
      border-radius: 6px;
      padding: 0 9px;
      background: #ffffff;
      color: var(--text);
      font: inherit;
      font-size: 12px;
    }

    .table-search:focus {
      outline: none;
      border-color: var(--accent);
      box-shadow: 0 0 0 3px rgba(23, 107, 135, 0.12);
    }

    .table-toggle {
      min-width: 58px;
      color: var(--muted);
      padding: 0 8px;
    }

    .table-group {
      display: grid;
      gap: 6px;
    }

    .group-title {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 8px;
      min-height: 28px;
      width: 100%;
      border: 0;
      border-radius: 4px;
      background: transparent;
      color: var(--muted);
      font-size: 11px;
      font-weight: 750;
      text-transform: uppercase;
      padding: 0 2px;
    }

    .group-title:hover {
      background: #eef3f6;
      border-color: transparent;
    }

    .group-title span:first-child {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .group-title span:last-child {
      white-space: nowrap;
      font-weight: 650;
    }

    .table-item {
      width: 100%;
      display: grid;
      grid-template-columns: minmax(0, 1fr) auto;
      align-items: center;
      gap: 8px;
      min-height: 32px;
      padding: 7px 9px;
      border: 1px solid var(--line);
      border-radius: 6px;
      background: var(--panel);
      color: var(--text);
      cursor: pointer;
      text-align: left;
      font: inherit;
    }

    .table-item:hover,
    .table-item.active {
      border-color: var(--accent);
    }

    .table-item.empty-table {
      color: var(--muted);
      background: #f7f9fb;
    }

    .table-name {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      font-size: 13px;
      font-weight: 650;
    }

    .row-count {
      color: var(--muted);
      font-size: 12px;
    }

    .schema {
      grid-template-rows: auto minmax(0, 1fr);
      gap: 8px;
    }

    .schema h2 {
      color: var(--text);
      font-size: 12px;
      text-transform: none;
    }

    .schema-row {
      display: grid;
      grid-template-columns: minmax(0, 1fr) auto;
      gap: 8px;
      padding: 6px 0;
      font-size: 12px;
      border-bottom: 1px solid #edf0f3;
    }

    .schema-row span:first-child {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .schema-row span:last-child {
      color: var(--muted);
    }

    #schemaRows {
      min-height: 0;
      overflow: auto;
      padding-right: 4px;
    }

    .query-panel {
      padding: 16px 20px;
      border-bottom: 1px solid var(--line);
      background: #ffffff;
      min-height: 210px;
      display: grid;
      grid-template-rows: auto minmax(110px, 1fr) auto;
      gap: 10px;
    }

    .quickbar {
      display: flex;
      gap: 8px;
      flex-wrap: wrap;
      align-items: center;
    }

    button {
      min-height: 34px;
      border: 1px solid var(--line);
      border-radius: 6px;
      background: #ffffff;
      color: var(--text);
      cursor: pointer;
      font: inherit;
      font-size: 13px;
      padding: 0 10px;
    }

    button:hover {
      border-color: var(--accent);
    }

    button.primary {
      background: var(--accent);
      color: #ffffff;
      border-color: var(--accent);
      font-weight: 650;
    }

    button.secondary {
      color: var(--accent-2);
    }

    textarea {
      width: 100%;
      height: 100%;
      min-height: 110px;
      resize: none;
      border: 1px solid var(--line);
      border-radius: 6px;
      background: var(--code);
      color: var(--code-text);
      padding: 12px;
      font: 13px/1.5 ui-monospace, SFMono-Regular, Consolas, "Liberation Mono", monospace;
      letter-spacing: 0;
      outline: none;
    }

    textarea:focus {
      border-color: var(--accent);
      box-shadow: 0 0 0 3px rgba(23, 107, 135, 0.14);
    }

    .actions {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 12px;
    }

    .limit {
      display: inline-flex;
      align-items: center;
      gap: 8px;
      color: var(--muted);
      font-size: 12px;
    }

    input[type="number"] {
      width: 86px;
      min-height: 32px;
      border: 1px solid var(--line);
      border-radius: 6px;
      padding: 0 8px;
      font: inherit;
    }

    .results {
      min-height: 0;
      overflow: auto;
      padding: 16px 20px 24px;
    }

    .result-meta {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 12px;
      margin-bottom: 10px;
      color: var(--muted);
      font-size: 12px;
    }

    .error {
      color: var(--danger);
      font-weight: 650;
    }

    .ok {
      color: var(--ok);
    }

    table {
      width: 100%;
      border-collapse: separate;
      border-spacing: 0;
      background: var(--panel);
      border: 1px solid var(--line);
      border-radius: 6px;
      overflow: hidden;
      box-shadow: var(--shadow);
      font-size: 12px;
    }

    th,
    td {
      border-bottom: 1px solid #eef1f4;
      padding: 8px 10px;
      text-align: left;
      vertical-align: top;
      white-space: nowrap;
      max-width: 360px;
      overflow: hidden;
      text-overflow: ellipsis;
    }

    th {
      position: sticky;
      top: 0;
      z-index: 1;
      background: #f0f3f6;
      color: #344150;
      font-weight: 700;
    }

    tr:last-child td {
      border-bottom: 0;
    }

    .empty {
      padding: 22px;
      border: 1px dashed var(--line);
      border-radius: 6px;
      background: #fff;
      color: var(--muted);
      font-size: 13px;
    }

    @media (max-width: 860px) {
      .app {
        grid-template-columns: 1fr;
      }

      aside {
        max-height: none;
        grid-template-rows: auto auto clamp(220px, 38vh, 360px) minmax(90px, 20vh);
        border-right: 0;
        border-bottom: 1px solid var(--line);
      }

      main {
        max-height: none;
        min-height: 66vh;
      }

      header {
        align-items: flex-start;
        flex-direction: column;
      }

      .status {
        text-align: left;
      }
    }
  </style>
</head>
<body>
  <div class="app">
    <aside>
      <div class="sidebar-brand">
        <h1>kat-rs Web UI</h1>
      </div>
      <section class="sidebar-section sidebar-source">
        <div class="section-head">
          <p class="section-title">Data source</p>
          <span class="section-kicker">Local</span>
        </div>
        <div id="traceMeta" class="trace-meta">Loading...</div>
        <div class="source-panel">
          <label class="field">
            <span>Dataset</span>
            <select id="datasetSelect"></select>
          </label>
          <label class="field">
            <span>Fixture</span>
            <select id="fixtureSelect"></select>
          </label>
          <div class="source-actions">
            <button id="openFixtureBtn">Open fixture</button>
          </div>
          <label class="field">
            <span>Upload</span>
            <input id="uploadInput" type="file">
          </label>
          <div class="source-actions">
            <button id="uploadBtn">Open upload</button>
          </div>
        </div>
      </section>
      <section class="sidebar-section table-browser">
        <div class="section-head">
          <p class="section-title">Tables</p>
          <span class="section-kicker">Browse</span>
        </div>
        <div class="table-toolbar">
          <input id="tableSearch" class="table-search" type="search" placeholder="Filter tables">
          <button id="toggleEmptyBtn" class="table-toggle">Empty</button>
        </div>
        <div id="tableList" class="table-list"></div>
      </section>
      <section class="sidebar-section schema">
        <div class="section-head">
          <h2 id="schemaTitle" class="section-title">Schema</h2>
        </div>
        <div id="schemaRows"></div>
      </section>
    </aside>

    <main>
      <header>
        <div>
          <h1 id="activeTitle">sched_slice</h1>
          <div id="activeMeta" class="trace-meta"></div>
        </div>
        <div id="status" class="status">Ready</div>
      </header>

      <section class="query-panel">
        <div class="quickbar">
          <button data-query="rows" title="Table row counts">Rows</button>
          <button data-query="preview" title="Preview active table">Preview</button>
          <button data-query="sched" title="CPU slices">CPU</button>
          <button data-query="threads" title="Threads">Threads</button>
          <button data-query="states" title="Thread states">States</button>
          <button data-query="raw" title="Raw events">Raw</button>
          <button data-query="irq" title="IRQ slices">IRQ</button>
          <button data-query="measure" title="Ftrace measures">Measures</button>
          <button data-query="cpuUsage" title="CPU plugin usage">CPU Usage</button>
          <button data-query="diskio" title="Disk IO plugin">Disk IO</button>
          <button data-query="logs" title="Hilog entries">Logs</button>
          <button data-query="hisysevent" title="HiSysEvent summary">HiSysEvent</button>
          <button data-query="hisyseventMeasure" title="HiSysEvent measure keys">Measures</button>
          <button data-query="perfSamples" title="Perf samples">Perf Samples</button>
          <button data-query="perfCallchain" title="Perf callchain frames">Callchain</button>
        </div>
        <textarea id="sql" spellcheck="false">SELECT cpu, COUNT(*) AS slices, SUM(dur) AS running_ns
FROM sched_slice
WHERE dur &gt; 0
GROUP BY cpu
ORDER BY cpu</textarea>
        <div class="actions">
          <label class="limit">Rows <input id="limit" type="number" min="1" max="10000" value="1000"></label>
          <div>
            <button id="copyBtn" class="secondary">Copy</button>
            <button id="runBtn" class="primary">Run</button>
          </div>
        </div>
      </section>

      <section class="results">
        <div class="result-meta">
          <span id="resultStatus">No result</span>
          <span id="resultCount"></span>
        </div>
        <div id="resultBody" class="empty">Run a query to view rows.</div>
      </section>
    </main>
  </div>

  <script>
    const tableGroups = [
      { label: 'Trace', names: ['trace_bounds', 'trace_metadata'] },
      { label: 'Scheduling', names: ['process', 'thread', 'sched_slice', 'thread_state'] },
      { label: 'Raw', names: ['raw_event', 'raw', 'instant'] },
      {
        label: 'Ftrace',
        names: ['irq', 'measure', 'measure_filter', 'cpu_measure_filter', 'symbols', 'dma_fence']
      },
      { label: 'Profiler', names: ['cpu_usage', 'diskio'] },
      { label: 'Logs', names: ['log'] },
      { label: 'HiSysEvent', names: ['hisysevent_all_event', 'hisysevent_measure'] },
      { label: 'Perf', names: ['perf_report', 'perf_files', 'perf_thread', 'perf_sample', 'perf_callchain'] }
    ];

    const staticQueries = {
      sched: `SELECT cpu, COUNT(*) AS slices, SUM(dur) AS running_ns
FROM sched_slice
WHERE dur > 0
GROUP BY cpu
ORDER BY cpu`,
      threads: `SELECT t.utid, t.tid, t.name, COUNT(s.utid) AS slices, SUM(s.dur) AS running_ns
FROM "thread" t
LEFT JOIN sched_slice s ON s.utid = t.utid
GROUP BY t.utid, t.tid, t.name
ORDER BY running_ns DESC NULLS LAST
LIMIT 100`,
      states: `SELECT state, COUNT(*) AS rows, SUM(dur) AS dur_ns
FROM thread_state
WHERE dur > 0
GROUP BY state
ORDER BY dur_ns DESC`,
      raw: `SELECT event_name, COUNT(*) AS rows, MIN(ts) AS first_ts, MAX(ts) AS last_ts
FROM raw_event
GROUP BY event_name
ORDER BY rows DESC, event_name
LIMIT 200`,
      irq: `SELECT cat, name, COUNT(*) AS rows, SUM(dur) AS dur_ns
FROM irq
GROUP BY cat, name
ORDER BY rows DESC, name
LIMIT 200`,
      measure: `SELECT mf.name, COUNT(*) AS rows, MIN(m.ts) AS first_ts, MAX(m.ts) AS last_ts
FROM measure m
LEFT JOIN measure_filter mf ON mf.id = m.filter_id
GROUP BY mf.name
ORDER BY rows DESC, mf.name
LIMIT 200`,
      cpuUsage: `SELECT ts, dur, total_load, user_load, system_load, process_num
FROM cpu_usage
ORDER BY ts
LIMIT 200`,
      diskio: `SELECT ts, dur, rd, wr, rd_speed, wr_speed, rd_count, wr_count
FROM diskio
ORDER BY ts
LIMIT 200`,
      logs: `SELECT ts, pid, tid, level, tag, context
FROM "log"
ORDER BY ts
LIMIT 200`,
      hisysevent: `SELECT domain, event_name, COUNT(*) AS events, MIN(ts) AS first_ts, MAX(ts) AS last_ts
FROM hisysevent_all_event
GROUP BY domain, event_name
ORDER BY events DESC, event_name
LIMIT 200`,
      hisyseventMeasure: `SELECT name, key, COUNT(*) AS rows, MIN(ts) AS first_ts, MAX(ts) AS last_ts
FROM hisysevent_measure
GROUP BY name, key
ORDER BY rows DESC, name, key
LIMIT 200`,
      perfSamples: `SELECT cpu_id, COUNT(*) AS samples, SUM(event_count) AS event_count
FROM perf_sample
GROUP BY cpu_id
ORDER BY samples DESC, cpu_id
LIMIT 200`,
      perfCallchain: `SELECT name, file_id, COUNT(*) AS frames
FROM perf_callchain
GROUP BY name, file_id
ORDER BY frames DESC, name
LIMIT 200`
    };

    const queries = {
      rows: () => buildRowCountsQuery(),
      preview: () => previewQuery(activeTable),
      ...staticQueries
    };

    const tableList = document.getElementById('tableList');
    const schemaRows = document.getElementById('schemaRows');
    const schemaTitle = document.getElementById('schemaTitle');
    const traceMeta = document.getElementById('traceMeta');
    const activeTitle = document.getElementById('activeTitle');
    const activeMeta = document.getElementById('activeMeta');
    const statusEl = document.getElementById('status');
    const resultStatus = document.getElementById('resultStatus');
    const resultCount = document.getElementById('resultCount');
    const resultBody = document.getElementById('resultBody');
    const sqlEl = document.getElementById('sql');
    const limitEl = document.getElementById('limit');
    const datasetSelect = document.getElementById('datasetSelect');
    const fixtureSelect = document.getElementById('fixtureSelect');
    const openFixtureBtn = document.getElementById('openFixtureBtn');
    const uploadInput = document.getElementById('uploadInput');
    const uploadBtn = document.getElementById('uploadBtn');
    const tableSearch = document.getElementById('tableSearch');
    const toggleEmptyBtn = document.getElementById('toggleEmptyBtn');

    let datasetsData = { datasets: [], fixtures: [], active_dataset_id: null };
    let activeDatasetId = null;
    let inspectData = null;
    let activeTable = null;
    let showEmptyTables = false;
    const collapsedGroups = new Set();

    function setStatus(text, className = '') {
      statusEl.textContent = text;
      statusEl.className = `status ${className}`;
    }

    function formatNumber(value) {
      return new Intl.NumberFormat().format(value ?? 0);
    }

    function formatBytes(value) {
      const bytes = Number(value || 0);
      if (bytes < 1024) return `${bytes} B`;
      if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
      return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
    }

    function quoteIdent(name) {
      return `"${String(name).replace(/"/g, '""')}"`;
    }

    function quoteString(value) {
      return `'${String(value).replace(/'/g, "''")}'`;
    }

    function datasetQuery() {
      return activeDatasetId ? `?dataset_id=${encodeURIComponent(activeDatasetId)}` : '';
    }

    async function refreshDatasets(preferredDatasetId = null) {
      const response = await fetch('/api/datasets');
      const payload = await response.json();
      if (!response.ok) {
        throw new Error(payload.error || 'Failed to list datasets');
      }
      datasetsData = payload;
      const datasetIds = payload.datasets.map(dataset => dataset.dataset_id);
      activeDatasetId = preferredDatasetId && datasetIds.includes(preferredDatasetId)
        ? preferredDatasetId
        : payload.active_dataset_id || datasetIds[0] || null;
      renderDatasetControls();
      return activeDatasetId;
    }

    function renderDatasetControls() {
      datasetSelect.innerHTML = datasetsData.datasets.length
        ? datasetsData.datasets.map(dataset => {
          const selected = dataset.dataset_id === activeDatasetId ? ' selected' : '';
          const kind = String(dataset.kind).replace('_', ' ');
          return `<option value="${escapeHtml(dataset.dataset_id)}"${selected}>${escapeHtml(dataset.label)} (${escapeHtml(kind)})</option>`;
        }).join('')
        : '<option value="">No dataset</option>';
      datasetSelect.disabled = datasetsData.datasets.length === 0;

      fixtureSelect.innerHTML = datasetsData.fixtures.length
        ? datasetsData.fixtures.map(fixture =>
          `<option value="${escapeHtml(fixture.path)}">${escapeHtml(fixture.path)} (${formatBytes(fixture.size_bytes)})</option>`
        ).join('')
        : '<option value="">No fixtures</option>';
      fixtureSelect.disabled = datasetsData.fixtures.length === 0;
      openFixtureBtn.disabled = datasetsData.fixtures.length === 0;
    }

    async function activateDataset(datasetId, options = { runDefaultQuery: true }) {
      if (!datasetId) {
        inspectData = null;
        activeDatasetId = null;
        traceMeta.textContent = 'No dataset open';
        tableList.innerHTML = '';
        schemaRows.innerHTML = '';
        schemaTitle.textContent = 'Schema';
        resultBody.className = 'empty';
        resultBody.textContent = 'No dataset.';
        setStatus('Ready');
        return;
      }
      activeDatasetId = datasetId;
      tableSearch.value = '';
      collapsedGroups.clear();
      renderDatasetControls();
      await loadInspect();
      if (options.runDefaultQuery) {
        await runQuery();
      }
    }

    function orderedTableNames() {
      const names = Object.keys(inspectData.tables);
      const seen = new Set();
      const ordered = [];

      tableGroups.forEach(group => {
        group.names.forEach(name => {
          if (inspectData.tables[name]) {
            seen.add(name);
            ordered.push(name);
          }
        });
      });

      names.sort().forEach(name => {
        if (!seen.has(name)) {
          ordered.push(name);
        }
      });

      return ordered;
    }

    function firstUsefulTable() {
      const names = orderedTableNames();
      return names.find(name => inspectData.tables[name].rows > 0) || names[0];
    }

    function categoryFor(name) {
      const group = tableGroups.find(item => item.names.includes(name));
      return group ? group.label : 'Other';
    }

    function tableMatchesFilter(name) {
      const needle = tableSearch.value.trim().toLowerCase();
      if (!needle) return true;
      return name.toLowerCase().includes(needle);
    }

    function buildRowCountsQuery() {
      const parts = orderedTableNames().map(name =>
        `SELECT ${quoteString(name)} AS table_name, COUNT(*) AS rows FROM ${quoteIdent(name)}`
      );
      return `SELECT *
FROM (
${parts.join('\nUNION ALL\n')}
) row_counts
ORDER BY rows DESC, table_name`;
    }

    function previewQuery(name) {
      const tableName = name || firstUsefulTable();
      return `SELECT *
FROM ${quoteIdent(tableName)}
LIMIT 200`;
    }

    function selectTable(name, options = {}) {
      if (!name || !inspectData.tables[name]) return;

      const table = inspectData.tables[name];
      activeTable = name;
      activeTitle.textContent = name;
      activeMeta.textContent = `${categoryFor(name)} | ${formatNumber(table.rows)} rows | ${formatNumber(table.columns.length)} columns`;
      schemaTitle.textContent = `${name} schema`;
      schemaRows.innerHTML = table.columns.map(column => `
        <div class="schema-row">
          <span title="${escapeHtml(column.name)}">${escapeHtml(column.name)}</span>
          <span>${escapeHtml(column.type)}</span>
        </div>
      `).join('');
      document.querySelectorAll('.table-item').forEach(item => {
        item.classList.toggle('active', item.dataset.table === name);
      });

      if (options.preview) {
        sqlEl.value = previewQuery(name);
        runQuery();
      }
    }

    function renderTableList() {
      const allNames = new Set(Object.keys(inspectData.tables));
      const rendered = new Set();
      const groups = tableGroups
        .map(group => ({
          label: group.label,
          names: group.names.filter(name => allNames.has(name))
        }))
        .filter(group => group.names.length > 0);

      groups.forEach(group => group.names.forEach(name => rendered.add(name)));
      const otherNames = [...allNames].filter(name => !rendered.has(name)).sort();
      if (otherNames.length) {
        groups.push({ label: 'Other', names: otherNames });
      }

      let visibleTableCount = 0;
      let hiddenEmptyCount = 0;
      const searchActive = tableSearch.value.trim().length > 0;
      const renderedGroups = groups.map(group => {
        const filteredNames = group.names.filter(name => {
          const table = inspectData.tables[name];
          if (!tableMatchesFilter(name)) return false;
          if (!showEmptyTables && table.rows === 0) {
            hiddenEmptyCount += 1;
            return false;
          }
          return true;
        });
        if (!filteredNames.length) return '';
        visibleTableCount += filteredNames.length;
        const rows = filteredNames.reduce((sum, name) => sum + inspectData.tables[name].rows, 0);
        const collapsed = collapsedGroups.has(group.label) && !searchActive;
        const chevron = collapsed ? '+' : '-';
        return `
          <div class="table-group">
            <button class="group-title" data-group="${escapeHtml(group.label)}" aria-expanded="${String(!collapsed)}">
              <span>${chevron} ${escapeHtml(group.label)}</span>
              <span>${formatNumber(filteredNames.length)} / ${formatNumber(rows)}</span>
            </button>
            ${collapsed ? '' : filteredNames.map(renderTableButton).join('')}
          </div>
        `;
      }).filter(Boolean);

      tableList.innerHTML = renderedGroups.length
        ? renderedGroups.join('')
        : `<div class="empty">No tables match.${hiddenEmptyCount ? ' Empty tables are hidden.' : ''}</div>`;
      toggleEmptyBtn.textContent = showEmptyTables ? 'All' : 'Non-empty';
      toggleEmptyBtn.title = showEmptyTables
        ? 'Showing all tables, including empty tables'
        : 'Showing only non-empty tables';
      tableSearch.placeholder = `Filter ${formatNumber(Object.keys(inspectData.tables).length)} tables`;
      traceMeta.dataset.visibleTables = String(visibleTableCount);

      tableList.querySelectorAll('.group-title').forEach(button => {
        button.addEventListener('click', () => {
          const label = button.dataset.group;
          if (!label) return;
          if (collapsedGroups.has(label)) {
            collapsedGroups.delete(label);
          } else {
            collapsedGroups.add(label);
          }
          renderTableList();
          if (activeTable) {
            document.querySelectorAll('.table-item').forEach(item => {
              item.classList.toggle('active', item.dataset.table === activeTable);
            });
          }
        });
      });
      tableList.querySelectorAll('.table-item').forEach(button => {
        button.addEventListener('click', () => selectTable(button.dataset.table, { preview: true }));
      });
    }

    function renderTableButton(name) {
      const table = inspectData.tables[name];
      const emptyClass = table.rows === 0 ? ' empty-table' : '';
      return `
        <button class="table-item${emptyClass}" data-table="${escapeHtml(name)}">
          <span class="table-name" title="${escapeHtml(name)}">${escapeHtml(name)}</span>
          <span class="row-count">${formatNumber(table.rows)}</span>
        </button>
      `;
    }

    async function loadInspect() {
      setStatus('Loading');
      if (!activeDatasetId) {
        await activateDataset(null, { runDefaultQuery: false });
        return;
      }
      const response = await fetch(`/api/inspect${datasetQuery()}`);
      inspectData = await response.json();
      if (!response.ok) {
        throw new Error(inspectData.error || 'Inspect failed');
      }
      const trace = inspectData.trace;
      const duration = trace.start_ts !== null && trace.end_ts !== null
        ? ` | dur ${formatNumber(trace.end_ts - trace.start_ts)} ns`
        : '';
      traceMeta.textContent = `${trace.label} | ${trace.clock_domain ?? '-'} | ${trace.start_ts ?? '-'} - ${trace.end_ts ?? '-'}${duration}`;
      traceMeta.title = (trace.paths || []).join('\n');

      renderTableList();
      selectTable(firstUsefulTable());
      sqlEl.value = buildRowCountsQuery();
      setStatus('Ready', 'ok');
    }

    async function runQuery() {
      if (!activeDatasetId) {
        resultStatus.textContent = 'No dataset open';
        resultStatus.className = 'error';
        resultCount.textContent = '';
        resultBody.className = 'empty';
        resultBody.textContent = 'No dataset.';
        setStatus('Ready');
        return;
      }
      setStatus('Running');
      resultStatus.textContent = 'Running';
      resultStatus.className = '';
      resultCount.textContent = '';
      resultBody.className = 'empty';
      resultBody.textContent = 'Running query...';

      try {
        const response = await fetch('/api/query', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            dataset_id: activeDatasetId,
            sql: sqlEl.value,
            max_inline_rows: Number(limitEl.value || 1000)
          })
        });
        const payload = await response.json();
        if (!response.ok) {
          throw new Error(payload.error || 'Query failed');
        }
        renderResult(payload);
        setStatus('Ready', 'ok');
      } catch (error) {
        resultStatus.textContent = error.message;
        resultStatus.className = 'error';
        resultCount.textContent = '';
        resultBody.className = 'empty';
        resultBody.textContent = 'No rows.';
        setStatus('Error', 'error');
      }
    }

    function renderResult(result) {
      resultStatus.textContent = result.status;
      resultStatus.className = result.status === 'ok' || result.status === 'empty_result' ? 'ok' : '';
      resultCount.textContent = `${formatNumber(result.stats.rows_returned)} rows${result.stats.truncated ? ' truncated' : ''}`;

      if (!result.rows.length) {
        resultBody.className = 'empty';
        resultBody.textContent = 'No rows.';
        return;
      }

      const columns = result.columns.map(column => column.name);
      resultBody.className = '';
      resultBody.innerHTML = `
        <table>
          <thead>
            <tr>${columns.map(column => `<th>${escapeHtml(column)}</th>`).join('')}</tr>
          </thead>
          <tbody>
            ${result.rows.map(row => `
              <tr>${columns.map(column => {
                const cell = formatCell(row[column]);
                return `<td title="${escapeHtml(cell)}">${escapeHtml(cell)}</td>`;
              }).join('')}</tr>
            `).join('')}
          </tbody>
        </table>
      `;
    }

    function formatCell(value) {
      if (value === null || value === undefined) return '';
      if (typeof value === 'object') return JSON.stringify(value);
      return String(value);
    }

    function escapeHtml(value) {
      return String(value).replace(/[&<>"']/g, char => ({
        '&': '&amp;',
        '<': '&lt;',
        '>': '&gt;',
        '"': '&quot;',
        "'": '&#39;'
      }[char]));
    }

    document.querySelectorAll('[data-query]').forEach(button => {
      button.addEventListener('click', () => {
        const query = queries[button.dataset.query];
        sqlEl.value = typeof query === 'function' ? query() : query;
      });
    });

    tableSearch.addEventListener('input', () => {
      if (inspectData) {
        renderTableList();
      }
    });

    toggleEmptyBtn.addEventListener('click', () => {
      showEmptyTables = !showEmptyTables;
      if (inspectData) {
        renderTableList();
      }
    });

    datasetSelect.addEventListener('change', () => {
      activateDataset(datasetSelect.value).catch(error => {
        resultStatus.textContent = error.message;
        resultStatus.className = 'error';
        setStatus('Error', 'error');
      });
    });

    openFixtureBtn.addEventListener('click', async () => {
      const path = fixtureSelect.value;
      if (!path) return;
      setStatus('Opening');
      openFixtureBtn.disabled = true;
      try {
        const response = await fetch('/api/datasets/fixture', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ path })
        });
        const payload = await response.json();
        if (!response.ok) {
          throw new Error(payload.error || 'Failed to open fixture');
        }
        await refreshDatasets(payload.dataset_id);
        await activateDataset(payload.dataset_id);
      } catch (error) {
        resultStatus.textContent = error.message;
        resultStatus.className = 'error';
        setStatus('Error', 'error');
      } finally {
        openFixtureBtn.disabled = fixtureSelect.disabled;
      }
    });

    uploadBtn.addEventListener('click', async () => {
      const file = uploadInput.files && uploadInput.files[0];
      if (!file) {
        setStatus('Choose file');
        return;
      }
      setStatus('Uploading');
      uploadBtn.disabled = true;
      try {
        const form = new FormData();
        form.append('trace', file, file.name);
        const response = await fetch('/api/datasets/upload', {
          method: 'POST',
          body: form
        });
        const payload = await response.json();
        if (!response.ok) {
          throw new Error(payload.error || 'Failed to upload trace');
        }
        uploadInput.value = '';
        await refreshDatasets(payload.dataset_id);
        await activateDataset(payload.dataset_id);
      } catch (error) {
        resultStatus.textContent = error.message;
        resultStatus.className = 'error';
        setStatus('Error', 'error');
      } finally {
        uploadBtn.disabled = false;
      }
    });

    document.getElementById('runBtn').addEventListener('click', runQuery);
    document.getElementById('copyBtn').addEventListener('click', async () => {
      await navigator.clipboard.writeText(sqlEl.value);
      setStatus('Copied', 'ok');
      setTimeout(() => setStatus('Ready', 'ok'), 900);
    });
    sqlEl.addEventListener('keydown', event => {
      if ((event.ctrlKey || event.metaKey) && event.key === 'Enter') {
        runQuery();
      }
    });

    refreshDatasets()
      .then(datasetId => activateDataset(datasetId))
      .catch(error => {
      resultStatus.textContent = error.message;
      resultStatus.className = 'error';
      setStatus('Error', 'error');
    });
  </script>
</body>
</html>
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/traces")
    }

    fn fixture_path() -> PathBuf {
        fixture_dir().join("ut_bytrace_input_full.txt")
    }

    fn thread_fixture_path() -> PathBuf {
        fixture_dir().join("ut_bytrace_input_thread.txt")
    }

    fn upload_dir() -> PathBuf {
        std::env::temp_dir().join(format!("kat-rs-web-ui-test-{}", unix_nanos()))
    }

    async fn test_state(trace_paths: Vec<PathBuf>) -> Arc<AppState> {
        build_state(trace_paths, fixture_dir(), upload_dir())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn inspect_endpoint_reports_sched_slice_from_datasource() {
        let state = test_state(vec![fixture_path()]).await;
        let Json(payload) = inspect(State(state), Query(DatasetQuery { dataset_id: None }))
            .await
            .unwrap();

        assert_eq!(payload["tables"]["sched_slice"]["rows"], 16);
        assert_eq!(payload["trace"]["trace_id"], "bytrace:b1aa5f38d23875c3");
    }

    #[tokio::test]
    async fn inspect_endpoint_reports_multiple_sources() {
        let state = test_state(vec![fixture_path(), thread_fixture_path()]).await;
        let Json(payload) = inspect(State(state), Query(DatasetQuery { dataset_id: None }))
            .await
            .unwrap();

        assert_eq!(payload["trace"]["sources"].as_array().unwrap().len(), 2);
        assert_eq!(payload["tables"]["sched_slice"]["rows"], 31);
    }

    #[tokio::test]
    async fn query_endpoint_returns_datasource_envelope_with_metrics() {
        let state = test_state(vec![fixture_path()]).await;
        let Json(result) = query(
            State(state),
            Json(QueryPayload {
                dataset_id: None,
                sql: "SELECT COUNT(*) AS slices FROM sched_slice".to_string(),
                max_inline_rows: 100,
            }),
        )
        .await
        .unwrap();

        assert_eq!(result.rows[0]["slices"], 16);
        assert!(result
            .metrics
            .phase_elapsed_ms
            .contains_key("query_execute"));
    }

    #[tokio::test]
    async fn datasets_endpoint_lists_fixture_files() {
        let state = test_state(Vec::new()).await;
        let Json(payload) = datasets(State(state)).await.unwrap();

        assert!(payload
            .fixtures
            .iter()
            .any(|fixture| fixture.path == "ut_bytrace_input_full.txt"));
        assert!(payload.datasets.is_empty());
        assert!(payload.active_dataset_id.is_none());
    }

    #[tokio::test]
    async fn open_fixture_endpoint_opens_and_activates_fixture_dataset() {
        let state = test_state(Vec::new()).await;
        let Json(opened) = open_fixture(
            State(Arc::clone(&state)),
            Json(OpenFixturePayload {
                path: "ut_bytrace_input_thread.txt".to_string(),
            }),
        )
        .await
        .unwrap();

        let Json(result) = query(
            State(state),
            Json(QueryPayload {
                dataset_id: Some(opened.dataset_id),
                sql: "SELECT COUNT(*) AS slices FROM sched_slice".to_string(),
                max_inline_rows: 100,
            }),
        )
        .await
        .unwrap();

        assert_eq!(result.rows[0]["slices"], 15);
    }

    #[tokio::test]
    async fn query_endpoint_can_select_non_active_dataset_id() {
        let state = test_state(vec![fixture_path()]).await;
        let Json(initial) = inspect(
            State(Arc::clone(&state)),
            Query(DatasetQuery { dataset_id: None }),
        )
        .await
        .unwrap();
        let current_dataset_id = initial["trace"]["dataset_id"].as_str().unwrap().to_string();

        let Json(opened) = open_fixture(
            State(Arc::clone(&state)),
            Json(OpenFixturePayload {
                path: "ut_bytrace_input_thread.txt".to_string(),
            }),
        )
        .await
        .unwrap();

        let Json(current_result) = query(
            State(Arc::clone(&state)),
            Json(QueryPayload {
                dataset_id: Some(current_dataset_id),
                sql: "SELECT COUNT(*) AS slices FROM sched_slice".to_string(),
                max_inline_rows: 100,
            }),
        )
        .await
        .unwrap();
        let Json(active_result) = query(
            State(state),
            Json(QueryPayload {
                dataset_id: Some(opened.dataset_id),
                sql: "SELECT COUNT(*) AS slices FROM sched_slice".to_string(),
                max_inline_rows: 100,
            }),
        )
        .await
        .unwrap();

        assert_eq!(current_result.rows[0]["slices"], 16);
        assert_eq!(active_result.rows[0]["slices"], 15);
    }

    #[tokio::test]
    async fn saved_upload_file_can_be_opened_and_queried() {
        let state = test_state(Vec::new()).await;
        fs::create_dir_all(&state.upload_dir).unwrap();
        let uploaded_path = state.upload_dir.join("uploaded-bytrace.txt");
        fs::copy(fixture_path(), &uploaded_path).unwrap();

        let dataset = open_web_dataset(
            &state.datasource,
            vec![uploaded_path],
            "Upload uploaded-bytrace.txt".to_string(),
            DatasetKind::Upload,
        )
        .await
        .unwrap();
        let dataset_id = dataset.handle.dataset_id.clone();
        insert_dataset(&state, dataset, true).unwrap();

        let Json(result) = query(
            State(state),
            Json(QueryPayload {
                dataset_id: Some(dataset_id),
                sql: "SELECT COUNT(*) AS slices FROM sched_slice".to_string(),
                max_inline_rows: 100,
            }),
        )
        .await
        .unwrap();

        assert_eq!(result.rows[0]["slices"], 16);
    }
}
