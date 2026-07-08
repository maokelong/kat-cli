use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result, anyhow, bail};
use kat_rs_datasource::TraceDatasource;
use serde_json::{Map, Value, json};

use crate::error::ApiError;

pub struct PackRunner {
    datasource: TraceDatasource,
    config: PackRunnerConfig,
}

impl PackRunner {
    pub fn new(datasource: TraceDatasource, config: PackRunnerConfig) -> Self {
        Self { datasource, config }
    }

    pub async fn run(&self, request: PackRunRequest) -> Result<PackRunSummary, ApiError> {
        self.run_inner(request)
            .await
            .map_err(|error| ApiError::validation(format!("{error:#}")))
    }

    async fn run_inner(&self, request: PackRunRequest) -> Result<PackRunSummary> {
        let mut child = Command::new(&self.config.python_executable)
            .arg("-I")
            .arg(&self.config.worker_script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| {
                format!(
                    "failed to start Python pack worker: {}",
                    self.config.python_executable.display()
                )
            })?;

        let mut child_stdin = child.stdin.take().context("worker stdin is not piped")?;
        let child_stdout = child.stdout.take().context("worker stdout is not piped")?;
        let mut stdout = BufReader::new(child_stdout);
        let mut registry = QueryRegistry::default();
        let mut logs = Vec::new();
        let mut completion = None;
        let mut traceback = None;

        write_json_line(
            &mut child_stdin,
            &json!({
                "kind": "run",
                "packRoot": path_string(&request.pack_root),
                "workflowName": request.workflow_name,
                "inputs": request.inputs,
                "sdkPath": path_string(&self.config.sdk_path),
                "runDir": path_string(&request.run_dir),
            }),
        )?;

        loop {
            let Some(message) = read_json_line(&mut stdout)? else {
                break;
            };
            match message.get("kind").and_then(Value::as_str) {
                Some("query") => {
                    let sql = required_str(&message, "sql")?.to_owned();
                    let params = message
                        .get("params")
                        .and_then(Value::as_object)
                        .cloned()
                        .unwrap_or_default();
                    let query_id = registry.insert(sql, params);
                    write_json_line(
                        &mut child_stdin,
                        &json!({ "kind": "queryResult", "queryId": query_id }),
                    )?;
                }
                Some("rows") => {
                    let query_id = required_str(&message, "queryId")?;
                    let max_rows = required_usize(&message, "maxRows")?;
                    match self.bounded_rows(&registry, query_id, max_rows).await {
                        Ok(rows) => write_json_line(
                            &mut child_stdin,
                            &json!({ "kind": "rowsResult", "rows": rows }),
                        )?,
                        Err(error) => write_failed_response(
                            &mut child_stdin,
                            format!("failed to read rows for query {query_id}: {error:#}"),
                        )?,
                    }
                }
                Some("preview") => {
                    let query_id = required_str(&message, "queryId")?;
                    let limit = required_usize(&message, "limit")?;
                    match self
                        .bounded_rows(&registry, query_id, limit.min(self.config.max_preview_rows))
                        .await
                    {
                        Ok(rows) => write_json_line(
                            &mut child_stdin,
                            &json!({ "kind": "rowsResult", "rows": rows }),
                        )?,
                        Err(error) => write_failed_response(
                            &mut child_stdin,
                            format!("failed to preview query {query_id}: {error:#}"),
                        )?,
                    }
                }
                Some("log") => {
                    logs.push(PackLogEntry {
                        level: required_str(&message, "level")?.to_owned(),
                        message: required_str(&message, "message")?.to_owned(),
                        fields: message.get("fields").cloned().unwrap_or_else(|| json!({})),
                    });
                    write_json_line(&mut child_stdin, &json!({ "kind": "logResult" }))?;
                }
                Some("complete") => {
                    completion = Some(message);
                    break;
                }
                Some("failed") => {
                    traceback = message
                        .get("traceback")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                    break;
                }
                Some(kind) => bail!("unknown worker message kind: {kind}"),
                None => bail!("worker message is missing kind: {message}"),
            }
        }

        drop(child_stdin);
        let status = child.wait().context("failed to wait for Python worker")?;
        let mut stderr = String::new();
        if let Some(mut pipe) = child.stderr.take() {
            pipe.read_to_string(&mut stderr)
                .context("failed to read Python worker stderr")?;
        }

        if let Some(traceback) = traceback {
            return Ok(PackRunSummary {
                status: PackRunStatus::Failed,
                artifacts: Vec::new(),
                logs,
                traceback: Some(traceback),
            });
        }

        if !status.success() {
            bail!("Python worker exited with {status}; stderr: {stderr}");
        }

        let completion = completion.context("Python worker exited without completion message")?;
        let artifacts = self
            .materialize_artifacts(&registry, &request.run_dir, &completion)
            .await?;

        Ok(PackRunSummary {
            status: PackRunStatus::Succeeded,
            artifacts,
            logs,
            traceback: None,
        })
    }

    async fn bounded_rows(
        &self,
        registry: &QueryRegistry,
        query_id: &str,
        max_rows: usize,
    ) -> Result<Vec<Value>> {
        if max_rows == 0 {
            bail!("max_rows must be positive");
        }
        if max_rows > self.config.max_bounded_rows {
            bail!(
                "requested rows {max_rows} exceeds configured bound {}",
                self.config.max_bounded_rows
            );
        }

        let record = registry.get(query_id)?;
        let rendered = render_sql_params(&record.sql, &record.params)?;
        let limited = format!(
            "select * from ({rendered}) as kat_query limit {}",
            max_rows + 1
        );
        let rows = query_rows(&self.datasource, &limited).await?;
        if rows.len() > max_rows {
            bail!("query {query_id} returned more than {max_rows} rows");
        }

        Ok(rows)
    }

    async fn materialize_artifacts(
        &self,
        registry: &QueryRegistry,
        run_dir: &Path,
        completion: &Value,
    ) -> Result<Vec<PackArtifactSummary>> {
        let artifacts = completion
            .get("artifacts")
            .and_then(Value::as_object)
            .context("complete message artifacts must be an object")?;
        let artifacts_dir = run_dir.join("artifacts");
        fs::create_dir_all(&artifacts_dir).with_context(|| {
            format!(
                "failed to create pack artifacts directory: {}",
                artifacts_dir.display()
            )
        })?;

        let mut summaries = Vec::new();
        for (name, query_id_value) in artifacts {
            validate_file_component(name, "artifact name")?;
            let query_id = query_id_value
                .as_str()
                .with_context(|| format!("artifact {name} query id must be a string"))?;
            let record = registry.get(query_id)?;
            let rendered = render_sql_params(&record.sql, &record.params)?;
            let rows = query_rows(&self.datasource, &rendered).await?;
            let row_count = rows.len();
            let preview = Value::Array(
                rows.iter()
                    .take(self.config.max_preview_rows)
                    .cloned()
                    .collect(),
            );

            let path = artifacts_dir.join(format!("{name}.json"));
            write_pretty_json(&path, &Value::Array(rows))?;

            let meta_path = artifacts_dir.join(format!("{name}.meta.json"));
            write_pretty_json(
                &meta_path,
                &json!({
                    "name": name,
                    "queryId": query_id,
                    "rowCount": row_count,
                }),
            )?;

            summaries.push(PackArtifactSummary {
                name: name.clone(),
                query_id: query_id.to_owned(),
                row_count,
                preview,
                path,
            });
        }

        Ok(summaries)
    }
}

#[derive(Clone, Debug)]
pub struct PackRunnerConfig {
    pub python_executable: PathBuf,
    pub worker_script: PathBuf,
    pub sdk_path: PathBuf,
    pub max_preview_rows: usize,
    pub max_bounded_rows: usize,
}

#[derive(Clone, Debug)]
pub struct PackRunRequest {
    pub pack_root: PathBuf,
    pub workflow_name: String,
    pub inputs: Map<String, Value>,
    pub run_dir: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PackRunStatus {
    Succeeded,
    Failed,
}

#[derive(Clone, Debug)]
pub struct PackRunSummary {
    pub status: PackRunStatus,
    pub artifacts: Vec<PackArtifactSummary>,
    pub logs: Vec<PackLogEntry>,
    pub traceback: Option<String>,
}

#[derive(Clone, Debug)]
pub struct PackArtifactSummary {
    pub name: String,
    pub query_id: String,
    pub row_count: usize,
    pub preview: Value,
    pub path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct PackLogEntry {
    pub level: String,
    pub message: String,
    pub fields: Value,
}

#[derive(Default)]
struct QueryRegistry {
    next_id: usize,
    records: BTreeMap<String, QueryRecord>,
}

impl QueryRegistry {
    fn insert(&mut self, sql: String, params: Map<String, Value>) -> String {
        self.next_id += 1;
        let query_id = format!("q{}", self.next_id);
        self.records
            .insert(query_id.clone(), QueryRecord { sql, params });
        query_id
    }

    fn get(&self, query_id: &str) -> Result<&QueryRecord> {
        self.records
            .get(query_id)
            .ok_or_else(|| anyhow!("unknown query id: {query_id}"))
    }
}

struct QueryRecord {
    sql: String,
    params: Map<String, Value>,
}

pub fn render_sql_params(sql: &str, params: &Map<String, Value>) -> Result<String> {
    let mut rendered = String::with_capacity(sql.len());
    let mut chars = sql.char_indices().peekable();
    let mut in_string = false;

    while let Some((_, ch)) = chars.next() {
        if ch == '\'' {
            rendered.push(ch);
            if in_string && matches!(chars.peek(), Some((_, '\''))) {
                let (_, escaped) = chars.next().expect("peeked escaped quote");
                rendered.push(escaped);
            } else {
                in_string = !in_string;
            }
            continue;
        }

        if !in_string && ch == ':' {
            let mut name = String::new();
            while let Some((_, next)) = chars.peek().copied() {
                if is_param_char(next, name.is_empty()) {
                    chars.next();
                    name.push(next);
                } else {
                    break;
                }
            }
            if name.is_empty() {
                rendered.push(ch);
            } else {
                let value = params
                    .get(&name)
                    .with_context(|| format!("missing SQL param: {name}"))?;
                rendered.push_str(&render_sql_value(value)?);
            }
            continue;
        }

        rendered.push(ch);
    }

    Ok(rendered)
}

fn render_sql_value(value: &Value) -> Result<String> {
    match value {
        Value::Null => Ok("NULL".to_owned()),
        Value::Bool(true) => Ok("TRUE".to_owned()),
        Value::Bool(false) => Ok("FALSE".to_owned()),
        Value::Number(number) => Ok(number.to_string()),
        Value::String(text) => Ok(format!("'{}'", text.replace('\'', "''"))),
        Value::Array(_) | Value::Object(_) => bail!("only scalar SQL params are supported"),
    }
}

fn is_param_char(ch: char, first: bool) -> bool {
    if first {
        ch == '_' || ch.is_ascii_alphabetic()
    } else {
        ch == '_' || ch.is_ascii_alphanumeric()
    }
}

async fn query_rows(datasource: &TraceDatasource, sql: &str) -> Result<Vec<Value>> {
    match datasource.query_json(sql).await? {
        Value::Array(rows) => Ok(rows),
        other => bail!("query returned non-array JSON value: {other}"),
    }
}

fn read_json_line(reader: &mut BufReader<impl Read>) -> Result<Option<Value>> {
    let mut line = String::new();
    let bytes = reader
        .read_line(&mut line)
        .context("failed to read worker message")?;
    if bytes == 0 {
        return Ok(None);
    }

    serde_json::from_str(&line).with_context(|| format!("failed to parse worker message: {line}"))
}

fn write_json_line(writer: &mut impl Write, value: &Value) -> Result<()> {
    serde_json::to_writer(&mut *writer, value).context("failed to write worker message")?;
    writer
        .write_all(b"\n")
        .context("failed to terminate worker message")?;
    writer.flush().context("failed to flush worker message")
}

fn write_failed_response(writer: &mut impl Write, message: String) -> Result<()> {
    write_json_line(
        writer,
        &json!({
            "kind": "failed",
            "message": message,
            "traceback": message,
        }),
    )
}

fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .with_context(|| format!("worker message field {key} must be a string"))
}

fn required_usize(value: &Value, key: &str) -> Result<usize> {
    let raw = value
        .get(key)
        .and_then(Value::as_u64)
        .with_context(|| format!("worker message field {key} must be a positive integer"))?;
    usize::try_from(raw).with_context(|| format!("worker message field {key} is too large"))
}

fn validate_file_component(value: &str, label: &str) -> Result<()> {
    if value.is_empty() {
        bail!("{label} must not be empty");
    }

    let path = Path::new(value);
    if !matches!(path.components().next(), Some(Component::Normal(_)))
        || path.components().count() != 1
    {
        bail!("{label} must be a single path component: {value}");
    }

    Ok(())
}

fn write_pretty_json(path: &Path, value: &Value) -> Result<()> {
    let file = File::create(path)
        .with_context(|| format!("failed to create JSON artifact: {}", path.display()))?;
    serde_json::to_writer_pretty(file, value)
        .with_context(|| format!("failed to write JSON artifact: {}", path.display()))
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
