use anyhow::{anyhow, Context, Result};
use clap::Parser;
use trace_parser::parse_trace_file;
use trace_query::{query_parsed_trace, QueryRequest};
use rusqlite::Connection;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

const TARGET_TABLES: &[&str] = &[
    "data_dict",
    "args",
    "callstack",
    "process_measure",
    "process_measure_filter",
    "sys_mem_measure",
    "sys_event_filter",
    "live_process",
    "js_heap_files",
    "js_heap_info",
    "js_heap_nodes",
    "js_heap_edges",
    "js_heap_string",
    "js_heap_location",
    "js_heap_sample",
    "js_heap_trace_function_info",
    "js_heap_trace_node",
    "js_config",
    "js_cpu_profiler_node",
    "js_cpu_profiler_sample",
];

#[derive(Debug, Parser)]
#[command(name = "compare-cpp-sqlite")]
#[command(about = "Compare Rust parsed tables with a TraceStreamer SQLite export")]
struct Cli {
    #[arg(long)]
    trace: Option<PathBuf>,
    #[arg(long = "cpp-db")]
    cpp_db: Option<PathBuf>,
    #[arg(long, default_value = "compare_validation_report.html")]
    html_output: PathBuf,
    #[arg(long)]
    json: bool,
}

struct ScenarioInput {
    name: String,
    trace: PathBuf,
    cpp_db: PathBuf,
}

#[derive(Debug, Serialize)]
struct CppTable {
    columns: Vec<String>,
    rows: u64,
}

#[derive(Debug, Serialize)]
struct TableComparison {
    table: String,
    cpp_rows: Option<u64>,
    rust_rows: Option<u64>,
    delta: Option<i64>,
    status: String,
}

struct AggregateCheck {
    name: &'static str,
    table: &'static str,
    sql: &'static str,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let scenarios = resolve_scenarios(&cli)?;
    let mut scenario_reports = Vec::new();
    for scenario in scenarios {
        scenario_reports.push(build_scenario_report(&scenario).await?);
    }

    let report = json!({
        "mode": "cpp_sqlite_vs_rust_parser",
        "target_tables": TARGET_TABLES,
        "scenarios": scenario_reports
    });

    let html = render_html_report(&report)?;
    fs::write(&cli.html_output, html)
        .with_context(|| format!("failed to write {}", cli.html_output.display()))?;

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    }

    let output = fs::canonicalize(&cli.html_output).unwrap_or_else(|_| cli.html_output.clone());
    println!("wrote {}", output.display());
    Ok(())
}

async fn build_scenario_report(scenario: &ScenarioInput) -> Result<Value> {
    let conn = Connection::open(&scenario.cpp_db)
        .with_context(|| format!("failed to open {}", scenario.cpp_db.display()))?;
    let parsed = parse_trace_file(&scenario.trace)
        .with_context(|| format!("failed to parse {}", scenario.trace.display()))?;

    let cpp_tables = cpp_tables(&conn)?;
    let rust_tables = rust_tables(&parsed);
    let table_comparison = compare_target_tables(&cpp_tables, &rust_tables);
    let all_table_comparison = compare_all_tables(&cpp_tables, &rust_tables);
    let aggregates = aggregate_report(&conn, &parsed).await;

    let summary = json!({
        "target_tables": TARGET_TABLES.len(),
        "row_count_matches": table_comparison.iter().filter(|item| item.status == "match").count(),
        "row_count_differences": table_comparison.iter().filter(|item| item.status == "different").count(),
        "missing_in_cpp": table_comparison.iter().filter(|item| item.status == "missing_in_cpp").count(),
        "missing_in_rust": table_comparison.iter().filter(|item| item.status == "missing_in_rust").count(),
        "all_tables": all_table_comparison.len(),
        "all_row_count_matches": all_table_comparison.iter().filter(|item| item.status == "match").count(),
        "all_row_count_differences": all_table_comparison.iter().filter(|item| item.status == "different").count(),
        "all_missing_in_cpp": all_table_comparison.iter().filter(|item| item.status == "missing_in_cpp").count(),
        "all_missing_in_rust": all_table_comparison.iter().filter(|item| item.status == "missing_in_rust").count(),
    });

    Ok(json!({
        "name": scenario.name.clone(),
        "inputs": {
            "trace": scenario.trace.display().to_string(),
            "cpp_db": scenario.cpp_db.display().to_string()
        },
        "summary": summary,
        "table_comparison": table_comparison,
        "all_table_comparison": all_table_comparison,
        "aggregates": aggregates,
        "cpp": {
            "tables": cpp_tables
        },
        "rust": {
            "trace_id": parsed.trace_id,
            "start_ts": parsed.start_ts,
            "end_ts": parsed.end_ts,
            "clock_domain": parsed.clock_domain,
            "tables": rust_tables
        }
    }))
}

fn resolve_scenarios(cli: &Cli) -> Result<Vec<ScenarioInput>> {
    if cli.trace.is_none() && cli.cpp_db.is_none() {
        let scenarios = default_scenarios();
        if !scenarios.is_empty() {
            return Ok(scenarios);
        }
    }

    let trace = cli
        .trace
        .clone()
        .or_else(default_trace)
        .ok_or_else(|| anyhow!("missing --trace and default pbreader.htrace was not found"))?;
    let cpp_db = cli.cpp_db.clone().or_else(default_cpp_db).ok_or_else(|| {
        anyhow!("missing --cpp-db and default cpp_htrace_pbreader.db was not found")
    })?;
    Ok(vec![ScenarioInput {
        name: "htrace".to_string(),
        trace,
        cpp_db,
    }])
}

fn default_scenarios() -> Vec<ScenarioInput> {
    let mut scenarios = Vec::new();
    if let (Some(trace), Some(cpp_db)) = (default_trace(), default_cpp_db()) {
        scenarios.push(ScenarioInput {
            name: "htrace_pbreader".to_string(),
            trace,
            cpp_db,
        });
    }
    if let (Some(trace), Some(cpp_db)) = (default_bytrace(), default_bytrace_cpp_db()) {
        scenarios.push(ScenarioInput {
            name: "bytrace_full".to_string(),
            trace,
            cpp_db,
        });
    }
    if let (Some(trace), Some(cpp_db)) = (default_perf(), default_perf_cpp_db()) {
        scenarios.push(ScenarioInput {
            name: "perf_compressed".to_string(),
            trace,
            cpp_db,
        });
    }
    scenarios
}

fn default_trace() -> Option<PathBuf> {
    let workspace = workspace_root();
    existing_path([
        workspace.join("tools/trace-validation/local_resource/pbreader.htrace"),
        workspace.join("tests/fixtures/traces/pbreader.htrace"),
    ])
}

fn default_cpp_db() -> Option<PathBuf> {
    let workspace = workspace_root();
    existing_path([workspace.join("target/cpp_htrace_pbreader.db")])
}

fn default_bytrace() -> Option<PathBuf> {
    let workspace = workspace_root();
    existing_path([
        workspace.join("tests/fixtures/traces/ut_bytrace_input_full.txt"),
        workspace.join("tools/trace-validation/local_resource/ut_bytrace_input_full.txt"),
    ])
}

fn default_bytrace_cpp_db() -> Option<PathBuf> {
    let workspace = workspace_root();
    existing_path([workspace.join("target/cpp_bytrace_full.db")])
}

fn default_perf() -> Option<PathBuf> {
    let workspace = workspace_root();
    existing_path([
        workspace.join("tests/fixtures/traces/perfCompressed.data"),
        workspace.join("tools/trace-validation/local_resource/perfCompressed.data"),
    ])
}

fn default_perf_cpp_db() -> Option<PathBuf> {
    let workspace = workspace_root();
    existing_path([workspace.join("target/cpp_perf_compressed.db")])
}

fn workspace_root() -> PathBuf {
    std::env::var_os("KAT_RS_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."))
}

fn existing_path(paths: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    paths.into_iter().find(|path| path.exists())
}

fn rust_tables(parsed: &trace_model::ParsedTrace) -> BTreeMap<String, Value> {
    parsed
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
                        .map(|field| field.name().to_string())
                        .collect::<Vec<_>>()
                }),
            )
        })
        .collect()
}

fn compare_target_tables(
    cpp_tables: &BTreeMap<String, CppTable>,
    rust_tables: &BTreeMap<String, Value>,
) -> Vec<TableComparison> {
    TARGET_TABLES
        .iter()
        .map(|table| {
            let cpp_rows = cpp_tables.get(*table).map(|table| table.rows);
            let rust_rows = rust_tables
                .get(*table)
                .and_then(|table| table.get("rows"))
                .and_then(Value::as_u64);
            let delta = match (cpp_rows, rust_rows) {
                (Some(cpp_rows), Some(rust_rows)) => Some(rust_rows as i64 - cpp_rows as i64),
                _ => None,
            };
            let status = match (cpp_rows, rust_rows) {
                (Some(cpp_rows), Some(rust_rows)) if cpp_rows == rust_rows => "match",
                (Some(_), Some(_)) => "different",
                (None, Some(_)) => "missing_in_cpp",
                (Some(_), None) => "missing_in_rust",
                (None, None) => "missing_in_both",
            }
            .to_string();
            TableComparison {
                table: (*table).to_string(),
                cpp_rows,
                rust_rows,
                delta,
                status,
            }
        })
        .collect()
}

fn compare_all_tables(
    cpp_tables: &BTreeMap<String, CppTable>,
    rust_tables: &BTreeMap<String, Value>,
) -> Vec<TableComparison> {
    let mut table_names = cpp_tables.keys().cloned().collect::<Vec<_>>();
    for table in rust_tables.keys() {
        if !cpp_tables.contains_key(table) {
            table_names.push(table.clone());
        }
    }
    table_names.sort();
    table_names
        .into_iter()
        .map(|table| compare_table(&table, cpp_tables, rust_tables))
        .collect()
}

fn compare_table(
    table: &str,
    cpp_tables: &BTreeMap<String, CppTable>,
    rust_tables: &BTreeMap<String, Value>,
) -> TableComparison {
    let cpp_rows = cpp_tables.get(table).map(|table| table.rows);
    let rust_rows = rust_tables
        .get(table)
        .and_then(|table| table.get("rows"))
        .and_then(Value::as_u64);
    let delta = match (cpp_rows, rust_rows) {
        (Some(cpp_rows), Some(rust_rows)) => Some(rust_rows as i64 - cpp_rows as i64),
        _ => None,
    };
    let status = match (cpp_rows, rust_rows) {
        (Some(cpp_rows), Some(rust_rows)) if cpp_rows == rust_rows => "match",
        (Some(_), Some(_)) => "different",
        (None, Some(_)) => "missing_in_cpp",
        (Some(_), None) => "missing_in_rust",
        (None, None) => "missing_in_both",
    }
    .to_string();
    TableComparison {
        table: table.to_string(),
        cpp_rows,
        rust_rows,
        delta,
        status,
    }
}

async fn aggregate_report(
    conn: &Connection,
    parsed: &trace_model::ParsedTrace,
) -> BTreeMap<String, Value> {
    let mut report = BTreeMap::new();
    for check in aggregate_checks() {
        let cpp = sqlite_rows_result(conn, check.table, check.sql);
        let rust = rust_query_result(parsed, check.sql).await;
        report.insert(
            check.name.to_string(),
            json!({
                "table": check.table,
                "sql": check.sql,
                "cpp": cpp,
                "rust": rust,
            }),
        );
    }
    report
}

fn aggregate_checks() -> Vec<AggregateCheck> {
    vec![
        AggregateCheck {
            name: "callstack_by_category",
            table: "callstack",
            sql: "SELECT cat, COUNT(*) AS rows, SUM(CASE WHEN dur IS NULL THEN 1 ELSE 0 END) AS null_dur FROM callstack GROUP BY cat ORDER BY rows DESC, cat",
        },
        AggregateCheck {
            name: "callstack_by_category_name",
            table: "callstack",
            sql: "SELECT cat, name, COUNT(*) AS rows, SUM(CASE WHEN dur IS NULL THEN 1 ELSE 0 END) AS null_dur FROM callstack GROUP BY cat, name ORDER BY rows DESC, cat, name LIMIT 1000",
        },
        AggregateCheck {
            name: "args_by_datatype",
            table: "args",
            sql: "SELECT datatype, COUNT(*) AS rows FROM args GROUP BY datatype ORDER BY datatype",
        },
        AggregateCheck {
            name: "args_detail_sample",
            table: "args",
            sql: "SELECT a.id, dk.data AS key_name, a.datatype, a.value, dv.data AS string_value, a.argset FROM args a LEFT JOIN data_dict dk ON a.key = dk.id LEFT JOIN data_dict dv ON a.value = dv.id ORDER BY a.id LIMIT 120",
        },
        AggregateCheck {
            name: "data_dict_sample",
            table: "data_dict",
            sql: "SELECT id, data FROM data_dict ORDER BY id LIMIT 200",
        },
        AggregateCheck {
            name: "process_measure_by_type",
            table: "process_measure",
            sql: "SELECT type, COUNT(*) AS rows, MIN(ts) AS min_ts, MAX(ts) AS max_ts FROM process_measure GROUP BY type ORDER BY rows DESC, type LIMIT 30",
        },
        AggregateCheck {
            name: "process_measure_filters",
            table: "process_measure_filter",
            sql: "SELECT name, COUNT(*) AS rows FROM process_measure_filter GROUP BY name ORDER BY rows DESC, name LIMIT 30",
        },
        AggregateCheck {
            name: "process_measure_rows_by_filter",
            table: "process_measure",
            sql: "SELECT f.name, COUNT(*) AS rows, MIN(m.ts) AS min_ts, MAX(m.ts) AS max_ts FROM process_measure m JOIN process_measure_filter f ON m.filter_id = f.id GROUP BY f.name ORDER BY rows DESC, f.name LIMIT 30",
        },
        AggregateCheck {
            name: "sys_mem_measure_by_type",
            table: "sys_mem_measure",
            sql: "SELECT type, COUNT(*) AS rows, MIN(ts) AS min_ts, MAX(ts) AS max_ts FROM sys_mem_measure GROUP BY type ORDER BY rows DESC, type LIMIT 30",
        },
        AggregateCheck {
            name: "sys_event_filters",
            table: "sys_event_filter",
            sql: "SELECT type, name, COUNT(*) AS rows FROM sys_event_filter GROUP BY type, name ORDER BY rows DESC, type, name LIMIT 30",
        },
        AggregateCheck {
            name: "live_process_by_process",
            table: "live_process",
            sql: "SELECT process_id, process_name, COUNT(*) AS rows, MIN(ts) AS min_ts, MAX(ts) AS max_ts FROM live_process GROUP BY process_id, process_name ORDER BY rows DESC, process_id LIMIT 30",
        },
        AggregateCheck {
            name: "js_heap_files",
            table: "js_heap_files",
            sql: "SELECT id, file_name, start_time, end_time, self_size FROM js_heap_files ORDER BY id LIMIT 20",
        },
        AggregateCheck {
            name: "js_heap_nodes_by_file",
            table: "js_heap_nodes",
            sql: "SELECT file_id, COUNT(*) AS rows, SUM(self_size) AS self_size FROM js_heap_nodes GROUP BY file_id ORDER BY file_id",
        },
        AggregateCheck {
            name: "js_heap_edges_by_file",
            table: "js_heap_edges",
            sql: "SELECT file_id, COUNT(*) AS rows FROM js_heap_edges GROUP BY file_id ORDER BY file_id",
        },
        AggregateCheck {
            name: "js_heap_strings_by_file",
            table: "js_heap_string",
            sql: "SELECT file_id, COUNT(*) AS rows FROM js_heap_string GROUP BY file_id ORDER BY file_id",
        },
        AggregateCheck {
            name: "js_config",
            table: "js_config",
            sql: "SELECT pid, type, interval, capture_numeric_value, trace_allocation, enable_cpu_profiler, cpu_profiler_interval FROM js_config ORDER BY pid LIMIT 20",
        },
        AggregateCheck {
            name: "js_cpu_profiler_node_sample",
            table: "js_cpu_profiler_node",
            sql: "SELECT function_id, function_index, script_id, url_index, line_number, column_number, hit_count, children, parent_id FROM js_cpu_profiler_node ORDER BY function_id LIMIT 20",
        },
        AggregateCheck {
            name: "js_cpu_profiler_sample_summary",
            table: "js_cpu_profiler_sample",
            sql: "SELECT function_id, COUNT(*) AS rows, MIN(start_time) AS min_start, MAX(end_time) AS max_end, SUM(dur) AS total_dur FROM js_cpu_profiler_sample GROUP BY function_id ORDER BY rows DESC, function_id LIMIT 30",
        },
    ]
}

fn cpp_tables(conn: &Connection) -> Result<BTreeMap<String, CppTable>> {
    let mut stmt =
        conn.prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")?;
    let table_names = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut tables = BTreeMap::new();
    for table in table_names {
        let columns = table_columns(conn, &table)?;
        let rows = conn.query_row(&format!("SELECT COUNT(*) FROM \"{table}\""), [], |row| {
            row.get::<_, u64>(0)
        })?;
        tables.insert(table, CppTable { columns, rows });
    }
    Ok(tables)
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info(\"{table}\")"))?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(columns)
}

fn sqlite_rows_result(conn: &Connection, table: &str, sql: &str) -> Value {
    match table_exists(conn, table) {
        Ok(true) => sqlite_rows(conn, sql)
            .unwrap_or_else(|err| json!({ "error": err.to_string(), "sql": sql })),
        Ok(false) => json!({ "error": format!("missing table {table}") }),
        Err(err) => json!({ "error": err.to_string() }),
    }
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [table],
        |row| row.get::<_, bool>(0),
    )?)
}

fn sqlite_rows(conn: &Connection, sql: &str) -> Result<Value> {
    let mut stmt = conn.prepare(sql)?;
    let names = stmt
        .column_names()
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let rows = stmt
        .query_map([], |row| {
            let mut object = serde_json::Map::new();
            for (index, name) in names.iter().enumerate() {
                object.insert(name.clone(), sqlite_value(row, index)?);
            }
            Ok(Value::Object(object))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Value::Array(rows))
}

fn sqlite_value(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Value> {
    let value = row.get_ref(index)?;
    Ok(match value {
        rusqlite::types::ValueRef::Null => Value::Null,
        rusqlite::types::ValueRef::Integer(value) => json!(value),
        rusqlite::types::ValueRef::Real(value) => json!(value),
        rusqlite::types::ValueRef::Text(value) => {
            json!(String::from_utf8_lossy(value).to_string())
        }
        rusqlite::types::ValueRef::Blob(value) => json!({ "blob_len": value.len() }),
    })
}

async fn rust_query_result(parsed: &trace_model::ParsedTrace, sql: &str) -> Value {
    rust_query(parsed, sql)
        .await
        .unwrap_or_else(|err| json!({ "error": err.to_string(), "sql": sql }))
}

async fn rust_query(parsed: &trace_model::ParsedTrace, sql: &str) -> Result<Value> {
    let result = query_parsed_trace(
        parsed,
        QueryRequest {
            sql: sql.to_string(),
            max_inline_rows: 100,
        },
    )
    .await?;
    Ok(json!({
        "columns": result.columns,
        "rows": result.rows,
        "stats": result.stats
    }))
}

fn render_html_report(report: &Value) -> Result<String> {
    let mut html = String::new();
    html.push_str("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    html.push_str("<title>TraceStreamer Rust Compare Validation</title>");
    html.push_str(
        "<style>
:root{color-scheme:light;--bg:#f3f5f8;--panel:#fff;--line:#d9dee8;--text:#20242a;--muted:#667085;--soft:#eef2f7;--good:#137333;--warn:#a15c00;--bad:#b3261e;--blue:#2458a6}
*{box-sizing:border-box}
body{font-family:Segoe UI,Arial,sans-serif;margin:0;color:var(--text);background:var(--bg);line-height:1.5}
.shell{max-width:1280px;margin:0 auto;padding:28px}
.hero{background:linear-gradient(135deg,#ffffff 0%,#edf3fb 100%);border:1px solid var(--line);border-radius:12px;padding:24px;margin-bottom:18px}
h1{font-size:28px;margin:0 0 8px;letter-spacing:0}
h2{font-size:18px;margin:0}
h3{font-size:15px;margin:22px 0 10px}
p{color:var(--muted);margin:0}
.scenario{background:var(--panel);border:1px solid var(--line);border-radius:12px;margin:18px 0;padding:18px;box-shadow:0 1px 2px rgba(16,24,40,.04)}
.scenario-head{display:flex;justify-content:space-between;gap:16px;align-items:flex-start;margin-bottom:16px}
.eyebrow{font-size:12px;text-transform:uppercase;letter-spacing:.08em;color:var(--blue);font-weight:700;margin-bottom:3px}
.path-grid{display:grid;grid-template-columns:1fr;gap:8px;margin:12px 0 16px}
.path-row{background:var(--soft);border:1px solid #e3e8f0;border-radius:8px;padding:9px 10px;font-size:12px;color:#354052;word-break:break-all}
.path-row strong{display:inline-block;min-width:52px;color:#1f2937}
.summary-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(150px,1fr));gap:10px;margin:12px 0 18px}
.metric{border:1px solid var(--line);border-radius:10px;padding:12px;background:#fbfcfe}
.metric-label{font-size:12px;color:var(--muted)}
.metric-value{font-size:24px;font-weight:700;margin-top:3px}
.table-wrap{overflow:auto;border:1px solid var(--line);border-radius:10px;background:var(--panel)}
table{border-collapse:collapse;width:100%;min-width:760px}
th,td{border-bottom:1px solid #e7ebf1;padding:9px 11px;text-align:left;font-size:13px;white-space:nowrap}
th{background:#f7f9fc;color:#344054;font-weight:700;position:sticky;top:0}
tr:last-child td{border-bottom:0}
.num{text-align:right;font-variant-numeric:tabular-nums}
.pill{display:inline-flex;align-items:center;border-radius:999px;padding:3px 9px;font-size:12px;font-weight:700}
.match{background:#e7f5ec;color:var(--good)}.different{background:#fff3dc;color:var(--warn)}.missing{background:#fdecec;color:var(--bad)}
.delta-pos{color:var(--good)}.delta-neg{color:var(--bad)}.delta-zero{color:var(--muted)}
.aggregate{border:1px solid var(--line);border-radius:10px;background:#fbfcfe;margin:8px 0}
.aggregate summary{cursor:pointer;padding:11px 12px;font-weight:700}
.aggregate pre,.raw-json pre{margin:0;border-top:1px solid var(--line);border-radius:0 0 10px 10px}
pre{background:#111827;color:#e5e7eb;padding:14px;overflow:auto;font-size:12px;line-height:1.45}
.raw-json{margin-top:20px}
.raw-json details{border:1px solid var(--line);border-radius:10px;background:var(--panel)}
.raw-json summary{padding:12px;font-weight:700;cursor:pointer}
</style></head><body>",
    );
    html.push_str("<div class=\"shell\"><section class=\"hero\"><p class=\"eyebrow\">TraceStreamer Rust</p><h1>Compare Validation Report</h1>");
    html.push_str("<p>C++ SQLite export compared with Rust parser output for shared dictionary/callstack, memory/process, and ArkTS JS heap table families.</p></section>");

    if let Some(scenarios) = report.get("scenarios").and_then(Value::as_array) {
        for scenario in scenarios {
            render_scenario_html(&mut html, scenario)?;
        }
    } else {
        render_scenario_html(&mut html, report)?;
    }

    html.push_str("<section class=\"raw-json\"><details><summary>Raw Report JSON</summary><pre>");
    html.push_str(&html_escape(&serde_json::to_string_pretty(report)?));
    html.push_str("</pre></details></section>");
    html.push_str("<script id=\"report-data\" type=\"application/json\">");
    html.push_str(&script_safe_json(report)?);
    html.push_str("</script></div></body></html>");
    Ok(html)
}

fn render_scenario_html(html: &mut String, scenario: &Value) -> Result<()> {
    let comparisons = scenario
        .get("table_comparison")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("scenario report missing table_comparison"))?;
    let all_comparisons = scenario
        .get("all_table_comparison")
        .and_then(Value::as_array)
        .unwrap_or(comparisons);
    let summary = scenario.get("summary").cloned().unwrap_or(Value::Null);
    let inputs = scenario.get("inputs").cloned().unwrap_or(Value::Null);
    let aggregates = scenario
        .get("aggregates")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("scenario report missing aggregates"))?;
    let name = scenario
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("single");

    html.push_str("<section class=\"scenario\"><div class=\"scenario-head\"><div><p class=\"eyebrow\">Scenario</p><h2>");
    html.push_str(&html_escape(name));
    html.push_str("</h2></div></div>");

    html.push_str("<div class=\"path-grid\">");
    if let Some(trace) = inputs.get("trace").and_then(Value::as_str) {
        html.push_str("<div class=\"path-row\"><strong>Trace</strong>");
        html.push_str(&html_escape(trace));
        html.push_str("</div>");
    }
    if let Some(cpp_db) = inputs.get("cpp_db").and_then(Value::as_str) {
        html.push_str("<div class=\"path-row\"><strong>C++ DB</strong>");
        html.push_str(&html_escape(cpp_db));
        html.push_str("</div>");
    }
    html.push_str("</div>");

    html.push_str("<div class=\"summary-grid\">");
    push_metric_card(html, "Target Tables", summary.get("target_tables"));
    push_metric_card(html, "Matches", summary.get("row_count_matches"));
    push_metric_card(html, "Differences", summary.get("row_count_differences"));
    push_metric_card(html, "Missing In Rust", summary.get("missing_in_rust"));
    push_metric_card(html, "All Tables", summary.get("all_tables"));
    push_metric_card(html, "All Matches", summary.get("all_row_count_matches"));
    push_metric_card(html, "All Diffs", summary.get("all_row_count_differences"));
    push_metric_card(html, "All Missing Rust", summary.get("all_missing_in_rust"));
    html.push_str("</div>");

    html.push_str("<h3>Target Table Row Counts</h3><div class=\"table-wrap\"><table><thead><tr><th>Table</th><th class=\"num\">C++ Rows</th><th class=\"num\">Rust Rows</th><th class=\"num\">Delta</th><th>Status</th></tr></thead><tbody>");
    render_comparison_rows(html, comparisons);
    html.push_str("</tbody></table></div>");

    html.push_str("<h3>All Table Row Counts</h3><div class=\"table-wrap\"><table><thead><tr><th>Table</th><th class=\"num\">C++ Rows</th><th class=\"num\">Rust Rows</th><th class=\"num\">Delta</th><th>Status</th></tr></thead><tbody>");
    render_comparison_rows(html, all_comparisons);
    html.push_str("</tbody></table></div>");

    html.push_str("<h3>Aggregate Checks</h3>");
    for (name, value) in aggregates {
        html.push_str("<details class=\"aggregate\"><summary>");
        html.push_str(&html_escape(name));
        html.push_str("</summary><pre>");
        html.push_str(&html_escape(&serde_json::to_string_pretty(value)?));
        html.push_str("</pre></details>");
    }
    html.push_str("</section>");
    Ok(())
}

fn render_comparison_rows(html: &mut String, comparisons: &[Value]) {
    for item in comparisons {
        let table = item.get("table").and_then(Value::as_str).unwrap_or("");
        let cpp_rows = value_to_cell(item.get("cpp_rows").unwrap_or(&Value::Null));
        let rust_rows = value_to_cell(item.get("rust_rows").unwrap_or(&Value::Null));
        let delta = value_to_cell(item.get("delta").unwrap_or(&Value::Null));
        let status = item.get("status").and_then(Value::as_str).unwrap_or("");
        let status_class = if status == "match" {
            "match"
        } else if status == "different" {
            "different"
        } else {
            "missing"
        };
        let delta_class = match item.get("delta").and_then(Value::as_i64) {
            Some(delta) if delta > 0 => "delta-pos",
            Some(delta) if delta < 0 => "delta-neg",
            Some(_) => "delta-zero",
            None => "",
        };
        html.push_str("<tr><td>");
        html.push_str(&html_escape(table));
        html.push_str("</td><td class=\"num\">");
        html.push_str(&html_escape(&cpp_rows));
        html.push_str("</td><td class=\"num\">");
        html.push_str(&html_escape(&rust_rows));
        html.push_str("</td><td class=\"num ");
        html.push_str(delta_class);
        html.push_str("\">");
        html.push_str(&html_escape(&delta));
        html.push_str("</td><td><span class=\"pill ");
        html.push_str(status_class);
        html.push_str("\">");
        html.push_str(&html_escape(status));
        html.push_str("</span></td></tr>");
    }
}

fn push_metric_card(html: &mut String, label: &str, value: Option<&Value>) {
    html.push_str("<div class=\"metric\"><div class=\"metric-label\">");
    html.push_str(&html_escape(label));
    html.push_str("</div><div class=\"metric-value\">");
    let value = value.map(value_to_cell).unwrap_or_else(|| "-".to_string());
    html.push_str(&html_escape(&value));
    html.push_str("</div></div>");
}

fn value_to_cell(value: &Value) -> String {
    match value {
        Value::Null => "-".to_string(),
        Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn script_safe_json(value: &Value) -> Result<String> {
    Ok(serde_json::to_string(value)?
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026"))
}
