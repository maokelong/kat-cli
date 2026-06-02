use crate::{ArtifactRef, DatasourceResult};
use serde_json::Value;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

pub struct ArtifactStore {
    root: PathBuf,
}

impl ArtifactStore {
    pub fn new(root: PathBuf) -> DatasourceResult<Self> {
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn write_jsonl(
        &self,
        dataset_id: &str,
        query_tag: Option<&str>,
        rows: &[Value],
    ) -> DatasourceResult<ArtifactRef> {
        let name_hash = stable_hash_hex(&format!(
            "{}\n{}\n{}",
            dataset_id,
            query_tag.unwrap_or("query"),
            rows.len()
        ));
        let path = self.root.join(format!("query-{name_hash}.jsonl"));
        let mut file = File::create(&path)?;
        let mut byte_size = 0_u64;

        for row in rows {
            let line = serde_json::to_string(row)?;
            file.write_all(line.as_bytes())?;
            file.write_all(b"\n")?;
            byte_size += line.len() as u64 + 1;
        }

        Ok(ArtifactRef {
            path: path.to_string_lossy().into_owned(),
            format: "jsonl".to_string(),
            row_count: rows.len(),
            byte_size,
            schema_hash: schema_hash(rows),
        })
    }
}

fn schema_hash(rows: &[Value]) -> String {
    let Some(Value::Object(first)) = rows.first() else {
        return stable_hash_hex("empty");
    };
    let mut keys = first.keys().cloned().collect::<Vec<_>>();
    keys.sort();
    stable_hash_hex(&keys.join("\n"))
}

fn stable_hash_hex(input: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}
