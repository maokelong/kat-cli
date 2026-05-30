use crate::config::models::AtomicResources;
use crate::engine::{EngineInfo, QueryEnvelope, QueryStats, TraceInfo, TraceQueryEngine};
use anyhow::{bail, Context, Result};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::NamedTempFile;

pub struct PerfettoShellEngine {
    binary: PathBuf,
}

impl PerfettoShellEngine {
    pub fn new(binary: PathBuf) -> Self {
        Self { binary }
    }
}

impl TraceQueryEngine for PerfettoShellEngine {
    fn query(
        &self,
        atomic_id: &str,
        trace_path: &Path,
        sql: &str,
        resources: &AtomicResources,
    ) -> Result<QueryEnvelope> {
        let query_file = NamedTempFile::new().context("创建查询文件")?;
        fs::write(query_file.path(), sql).context("写入查询文件")?;
        let output = Command::new(&self.binary)
            .arg("-q")
            .arg(query_file.path())
            .arg(trace_path)
            .output()
            .with_context(|| format!("运行 {}", self.binary.display()))?;

        if !output.status.success() {
            bail!(
                "trace_processor 查询失败: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut row = BTreeMap::new();
        row.insert("raw_stdout".to_string(), json!(stdout.trim()));
        Ok(QueryEnvelope {
            status: "ok".to_string(),
            atomic_id: atomic_id.to_string(),
            engine: EngineInfo {
                name: "perfetto-shell".to_string(),
                version: "external".to_string(),
            },
            trace: TraceInfo {
                path: trace_path.display().to_string(),
            },
            rows: vec![row],
            artifacts: vec![],
            stats: QueryStats {
                rows_returned: 1.min(resources.max_rows),
                truncated: stdout.len() > resources.max_result_bytes,
            },
        })
    }
}
