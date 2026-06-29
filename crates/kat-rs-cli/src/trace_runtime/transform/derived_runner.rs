use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::trace_runtime::{
    adapter::DatasetAdapter,
    pack::{LoadedPack, spec::TransformSpec},
    transform::run_transform,
};

#[derive(Debug)]
pub struct DerivedRunner<'a> {
    pack: &'a LoadedPack,
    by_output_table: BTreeMap<String, &'a TransformSpec>,
    materialized: BTreeMap<(usize, String), MaterializationFingerprint>,
    metadata: BTreeMap<String, DerivedTableMetadata>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DerivedTableMetadata {
    pub pack_id: String,
    pub transform_id: String,
    pub input_tables: Vec<String>,
    pub output_table: String,
    pub output_schema: String,
    pub semantic: Option<String>,
    pub materialize: Option<String>,
    pub backend: String,
}

#[derive(Clone, Debug, PartialEq)]
struct MaterializationFingerprint {
    params: Value,
    state: Option<Value>,
}

impl<'a> DerivedRunner<'a> {
    pub fn new(pack: &'a LoadedPack) -> Result<Self> {
        let mut by_output_table = BTreeMap::new();
        for transform in &pack.transforms {
            if let Some(existing) =
                by_output_table.insert(transform.output().table.clone(), transform)
            {
                bail!(
                    "duplicate transform output table `{}` produced by `{}` and `{}`",
                    transform.output().table,
                    existing.id(),
                    transform.id()
                );
            }
        }
        Ok(Self {
            pack,
            by_output_table,
            materialized: BTreeMap::new(),
            metadata: BTreeMap::new(),
        })
    }

    pub fn ensure_table(
        &mut self,
        adapter: &mut dyn DatasetAdapter,
        table: &str,
        params: &Value,
        state: &Value,
    ) -> Result<()> {
        let mut visiting = BTreeSet::new();
        let adapter_id = adapter_identity(adapter);
        self.ensure_table_inner(adapter, adapter_id, table, params, state, &mut visiting)
    }

    pub fn materialized_metadata(&self, table: &str) -> Option<&DerivedTableMetadata> {
        self.metadata.get(table)
    }

    fn ensure_table_inner(
        &mut self,
        adapter: &mut dyn DatasetAdapter,
        adapter_id: usize,
        table: &str,
        params: &Value,
        state: &Value,
        visiting: &mut BTreeSet<String>,
    ) -> Result<()> {
        if adapter.table_exists(table)? {
            if !self.by_output_table.contains_key(table) {
                return Ok(());
            }
            let transform = self
                .by_output_table
                .get(table)
                .expect("output table index was checked");
            if let Some(existing) = self.materialized.get(&(adapter_id, table.to_string())) {
                let requested = self.materialization_fingerprint(table, params, state)?;
                if existing == &requested {
                    return Ok(());
                }
                bail!(
                    "derived table `{table}` was already materialized with different params/state"
                );
            }
            bail!(
                "derived table `{table}` already exists for transform `{}` but was not materialized by this runner",
                transform.id()
            );
        }

        let Some(transform) = self.by_output_table.get(table).copied() else {
            bail!("table `{table}` does not exist and is not produced by a pack transform");
        };

        if !visiting.insert(table.to_string()) {
            bail!("cycle while materializing derived table `{table}`");
        }

        for input in transform.inputs().table_names() {
            if self.by_output_table.contains_key(input) {
                self.ensure_table_inner(adapter, adapter_id, input, params, state, visiting)?;
            } else if !adapter.table_exists(input)? {
                bail!(
                    "transform `{}` input table `{input}` does not exist and is not produced by a pack transform",
                    transform.id()
                );
            }
        }

        run_transform(adapter, self.pack, transform, params, state)
            .with_context(|| format!("failed to run transform `{}`", transform.id()))?;
        let fingerprint = self.materialization_fingerprint(table, params, state)?;
        self.materialized
            .insert((adapter_id, table.to_string()), fingerprint);
        let metadata = DerivedTableMetadata {
            pack_id: self.pack.manifest.id.clone(),
            transform_id: transform.id().to_string(),
            input_tables: transform
                .inputs()
                .table_names()
                .into_iter()
                .map(str::to_string)
                .collect(),
            output_table: transform.output().table.clone(),
            output_schema: transform.output().schema.clone(),
            semantic: transform.output().semantic.clone(),
            materialize: transform.materialize().map(str::to_string),
            backend: "sqlite-prototype".to_string(),
        };
        self.metadata.insert(table.to_string(), metadata);
        visiting.remove(table);
        Ok(())
    }

    fn materialization_fingerprint(
        &self,
        table: &str,
        params: &Value,
        state: &Value,
    ) -> Result<MaterializationFingerprint> {
        Ok(MaterializationFingerprint {
            params: params.clone(),
            state: self
                .table_uses_state_transitively(table)?
                .then(|| state.clone()),
        })
    }

    fn table_uses_state_transitively(&self, table: &str) -> Result<bool> {
        let mut visiting = BTreeSet::new();
        self.table_uses_state_transitively_inner(table, &mut visiting)
    }

    fn table_uses_state_transitively_inner(
        &self,
        table: &str,
        visiting: &mut BTreeSet<String>,
    ) -> Result<bool> {
        let Some(transform) = self.by_output_table.get(table).copied() else {
            return Ok(false);
        };

        if !visiting.insert(table.to_string()) {
            bail!("cycle while checking derived table state sensitivity for `{table}`");
        }

        let uses_state = transform_uses_state(transform)
            || transform
                .inputs()
                .table_names()
                .into_iter()
                .map(|input| self.table_uses_state_transitively_inner(input, visiting))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .any(|uses_state| uses_state);

        visiting.remove(table);
        Ok(uses_state)
    }
}

fn transform_uses_state(transform: &TransformSpec) -> bool {
    transform.uses_state_template()
}

fn adapter_identity(adapter: &mut dyn DatasetAdapter) -> usize {
    adapter as *mut dyn DatasetAdapter as *mut () as usize
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::trace_runtime::pack::PackManifest;

    use super::*;

    #[test]
    fn metadata_map_is_keyed_by_table_name() {
        let pack = LoadedPack {
            root: PathBuf::new(),
            manifest: PackManifest {
                id: "metadata-pack".to_string(),
                name: None,
                schemas: Vec::new(),
                derived: Vec::new(),
                queries: Vec::new(),
                analyses: Vec::new(),
                rules: Vec::new(),
            },
            transforms: Vec::new(),
            analyses: Vec::new(),
            rule_sets: Vec::new(),
        };
        let runner = DerivedRunner::new(&pack).expect("runner is created");

        assert!(runner.metadata.get("missing_table").is_none());
    }
}
