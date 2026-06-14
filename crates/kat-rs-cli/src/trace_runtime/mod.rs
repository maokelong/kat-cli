use std::{fs, io::Write, path::PathBuf};

use anyhow::{Context, Result};
use serde_json::Value;

pub mod evidence;
pub mod manifest;
pub mod operators;
pub mod query_client;
pub mod runtime;
pub mod sqlite_query_client;

#[derive(Clone, Copy, Debug, Eq, PartialEq, clap::ValueEnum)]
pub enum ProbeSourceArg {
    Sqlite,
}

#[derive(Clone, Debug)]
pub struct ProbeRunOptions {
    pub probe: String,
    pub source: ProbeSourceArg,
    pub file: PathBuf,
    pub params_file: PathBuf,
    pub run_dir: Option<PathBuf>,
}

pub async fn run_probe(options: ProbeRunOptions, out: &mut dyn Write) -> Result<()> {
    let registry = manifest::registry_root();
    let manifest = manifest::load_manifest(&registry, &options.probe)?;
    let probe_dir = registry.join(&options.probe);
    let params = read_params(&options, &manifest)?;

    let evidence = match options.source {
        ProbeSourceArg::Sqlite => {
            let mut client = sqlite_query_client::SqliteQueryClient::open(&options.file)
                .with_context(|| format!("failed to open sqlite {}", options.file.display()))?;
            runtime::run_manifest(&manifest, &probe_dir, params, &mut client)?
        }
    };

    evidence::write_stdout(out, &evidence)?;
    if let Some(run_dir) = options.run_dir {
        evidence::append_jsonl(run_dir, &evidence)?;
    }

    Ok(())
}

fn read_params(options: &ProbeRunOptions, manifest: &manifest::ProbeManifest) -> Result<Value> {
    let raw = fs::read_to_string(&options.params_file)
        .with_context(|| format!("failed to read {}", options.params_file.display()))?;
    let raw = raw.trim_start_matches('\u{feff}');
    let mut params: Value = serde_json::from_str(raw)
        .with_context(|| format!("failed to parse {}", options.params_file.display()))?;

    if manifest.inputs.contains_key("db") {
        let object = params
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("probe params must be a JSON object"))?;
        object
            .entry("db".to_string())
            .or_insert_with(|| Value::String(options.file.to_string_lossy().into_owned()));
    }

    Ok(params)
}
