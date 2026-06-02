use crate::{DatasourceResult, TraceSource};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub const PARSER_VERSION: &str = "trace-parser.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatasetCacheKey {
    pub schema_version: String,
    pub parser_version: String,
    pub sources: Vec<SourceCacheKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceCacheKey {
    pub path: String,
    pub len: u64,
    pub modified_unix_ms: Option<u128>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetCacheManifest {
    pub dataset_id: String,
    pub schema_version: String,
    pub parser_version: String,
    pub source_count: usize,
    pub cache_key: DatasetCacheKey,
}

pub fn build_dataset_cache_key(
    schema_version: &str,
    sources: &[TraceSource],
) -> DatasourceResult<DatasetCacheKey> {
    let mut source_keys = sources
        .iter()
        .map(|source| source_cache_key(&source.path))
        .collect::<DatasourceResult<Vec<_>>>()?;
    source_keys.sort_by(|left, right| left.path.cmp(&right.path));

    Ok(DatasetCacheKey {
        schema_version: schema_version.to_string(),
        parser_version: PARSER_VERSION.to_string(),
        sources: source_keys,
    })
}

pub fn dataset_cache_manifest_path(
    cache_dir: &Path,
    cache_key: &DatasetCacheKey,
) -> DatasourceResult<PathBuf> {
    let encoded = serde_json::to_string(cache_key)?;
    Ok(cache_dir
        .join("datasets")
        .join(format!("{}.manifest.json", stable_hash_hex(&encoded))))
}

pub fn write_dataset_cache_manifest(
    cache_dir: &Path,
    dataset_id: &str,
    cache_key: &DatasetCacheKey,
) -> DatasourceResult<PathBuf> {
    let path = dataset_cache_manifest_path(cache_dir, cache_key)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let manifest = DatasetCacheManifest {
        dataset_id: dataset_id.to_string(),
        schema_version: cache_key.schema_version.clone(),
        parser_version: cache_key.parser_version.clone(),
        source_count: cache_key.sources.len(),
        cache_key: cache_key.clone(),
    };
    fs::write(&path, serde_json::to_vec_pretty(&manifest)?)?;
    Ok(path)
}

fn source_cache_key(path: &Path) -> DatasourceResult<SourceCacheKey> {
    let metadata = fs::metadata(path)?;
    let modified_unix_ms = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis());
    let normalized_path = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned();

    Ok(SourceCacheKey {
        path: normalized_path,
        len: metadata.len(),
        modified_unix_ms,
    })
}

fn stable_hash_hex(input: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}
