use anyhow::{Context, Result};
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use htrace_core::QueryRequest;
use htrace_parser_harmony::parse_trace_file;
use htrace_query::query_parsed_trace;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{net::SocketAddr, path::PathBuf, sync::Arc};

#[derive(Debug, Parser)]
#[command(name = "htrace-web-ui")]
#[command(about = "Small isolated local web UI for querying parsed htrace data")]
struct Cli {
    #[arg(long)]
    trace: PathBuf,
    #[arg(long, default_value = "127.0.0.1:8787")]
    listen: SocketAddr,
}

#[derive(Clone)]
struct AppState {
    trace_path: PathBuf,
    parsed: Arc<htrace_model::ParsedTrace>,
}

#[derive(Debug, Deserialize)]
struct QueryPayload {
    sql: String,
    #[serde(default = "default_max_inline_rows")]
    max_inline_rows: usize,
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
    let parsed = parse_trace_file(&cli.trace)
        .with_context(|| format!("failed to parse {}", cli.trace.display()))?;

    let state = Arc::new(AppState {
        trace_path: cli.trace,
        parsed: Arc::new(parsed),
    });

    let app = Router::new()
        .route("/", get(index))
        .route("/api/inspect", get(inspect))
        .route("/api/query", post(query))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(cli.listen).await?;
    println!("htrace-web-ui listening on http://{}", cli.listen);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn inspect(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let tables = state
        .parsed
        .tables
        .batches()
        .into_iter()
        .map(|(name, batch)| {
            (
                name.to_string(),
                json!({
                    "rows": batch.num_rows(),
                    "columns": batch
                        .schema()
                        .fields()
                        .iter()
                        .map(|field| json!({
                            "name": field.name(),
                            "type": field.data_type().to_string()
                        }))
                        .collect::<Vec<_>>()
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();

    Json(json!({
        "trace": {
            "path": state.trace_path,
            "trace_id": state.parsed.trace_id,
            "start_ts": state.parsed.start_ts,
            "end_ts": state.parsed.end_ts,
            "clock_domain": state.parsed.clock_domain
        },
        "tables": tables
    }))
}

async fn query(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<QueryPayload>,
) -> Result<Json<htrace_core::QueryResult>, ApiError> {
    if payload.sql.trim().is_empty() {
        return Err(ApiError::bad_request("SQL is empty"));
    }

    let result = query_parsed_trace(
        &state.parsed,
        QueryRequest {
            sql: payload.sql,
            max_inline_rows: payload.max_inline_rows,
        },
    )
    .await
    .map_err(|err| ApiError::bad_request(err.to_string()))?;

    Ok(Json(result))
}

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
  <title>HTrace Query</title>
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
      grid-template-columns: minmax(220px, 300px) minmax(0, 1fr);
      min-height: 100vh;
    }

    aside {
      border-right: 1px solid var(--line);
      background: #fbfcfd;
      padding: 18px;
      overflow: auto;
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
      margin-top: 4px;
      color: var(--muted);
      font-size: 12px;
      overflow-wrap: anywhere;
    }

    .status {
      min-width: 120px;
      text-align: right;
      color: var(--muted);
      font-size: 12px;
    }

    .table-list {
      display: grid;
      gap: 14px;
      margin-top: 14px;
    }

    .table-group {
      display: grid;
      gap: 7px;
    }

    .group-title {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 8px;
      color: var(--muted);
      font-size: 11px;
      font-weight: 750;
      text-transform: uppercase;
    }

    .table-item {
      width: 100%;
      display: grid;
      grid-template-columns: minmax(0, 1fr) auto;
      align-items: center;
      gap: 8px;
      padding: 10px;
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
      margin-top: 16px;
      border-top: 1px solid var(--line);
      padding-top: 14px;
    }

    .schema h2 {
      margin: 0 0 8px;
      font-size: 13px;
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
        max-height: 34vh;
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
      <h1>HTrace Query</h1>
      <div id="traceMeta" class="trace-meta">Loading...</div>
      <div id="tableList" class="table-list"></div>
      <div class="schema">
        <h2 id="schemaTitle">Schema</h2>
        <div id="schemaRows"></div>
      </div>
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

    let inspectData = null;
    let activeTable = null;

    function setStatus(text, className = '') {
      statusEl.textContent = text;
      statusEl.className = `status ${className}`;
    }

    function formatNumber(value) {
      return new Intl.NumberFormat().format(value ?? 0);
    }

    function quoteIdent(name) {
      return `"${String(name).replace(/"/g, '""')}"`;
    }

    function quoteString(value) {
      return `'${String(value).replace(/'/g, "''")}'`;
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

      tableList.innerHTML = groups.map(group => {
        const rows = group.names.reduce((sum, name) => sum + inspectData.tables[name].rows, 0);
        return `
          <div class="table-group">
            <div class="group-title">
              <span>${escapeHtml(group.label)}</span>
              <span>${formatNumber(rows)}</span>
            </div>
            ${group.names.map(renderTableButton).join('')}
          </div>
        `;
      }).join('');

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
      const response = await fetch('/api/inspect');
      inspectData = await response.json();
      const trace = inspectData.trace;
      const duration = trace.start_ts !== null && trace.end_ts !== null
        ? ` | dur ${formatNumber(trace.end_ts - trace.start_ts)} ns`
        : '';
      traceMeta.textContent = `${trace.clock_domain} | ${trace.start_ts ?? '-'} - ${trace.end_ts ?? '-'}${duration}`;
      traceMeta.title = trace.path || '';

      renderTableList();
      selectTable(firstUsefulTable());
      sqlEl.value = buildRowCountsQuery();
      setStatus('Ready', 'ok');
    }

    async function runQuery() {
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

    loadInspect().then(runQuery).catch(error => {
      resultStatus.textContent = error.message;
      resultStatus.className = 'error';
      setStatus('Error', 'error');
    });
  </script>
</body>
</html>
"#;
