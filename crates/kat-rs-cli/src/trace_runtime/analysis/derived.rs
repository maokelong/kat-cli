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
                by_output_table.insert(transform.output.table.clone(), transform)
            {
                bail!(
                    "duplicate transform output table `{}` produced by `{}` and `{}`",
                    transform.output.table,
                    existing.id,
                    transform.id
                );
            }
        }
        Ok(Self {
            pack,
            by_output_table,
            materialized: BTreeMap::new(),
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
                transform.id
            );
        }

        let Some(transform) = self.by_output_table.get(table).copied() else {
            bail!("table `{table}` does not exist and is not produced by a pack transform");
        };

        if !visiting.insert(table.to_string()) {
            bail!("cycle while materializing derived table `{table}`");
        }

        for input in transform.inputs.table_names() {
            if self.by_output_table.contains_key(input) {
                self.ensure_table_inner(adapter, adapter_id, input, params, state, visiting)?;
            } else if !adapter.table_exists(input)? {
                bail!(
                    "transform `{}` input table `{input}` does not exist and is not produced by a pack transform",
                    transform.id
                );
            }
        }

        run_transform(adapter, self.pack, transform, params, state)
            .with_context(|| format!("failed to run transform `{}`", transform.id))?;
        let fingerprint = self.materialization_fingerprint(table, params, state)?;
        self.materialized
            .insert((adapter_id, table.to_string()), fingerprint);
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
                .inputs
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
    string_uses_state(&transform.kind)
        || transform
            .params
            .values()
            .any(|value| string_uses_state(value))
        || transform
            .bind
            .values()
            .any(|value| string_uses_state(value))
        || transform
            .where_
            .values()
            .any(|value| value_uses_state(value))
        || transform.source.as_ref().is_some_and(|source| {
            string_uses_state(&source.table)
                || string_uses_state(&source.column)
                || string_uses_state(&source.contains)
        })
        || transform
            .fields
            .values()
            .any(|value| string_uses_state(value))
        || transform.joins.iter().any(|(key, values)| {
            string_uses_state(key)
                || values
                    .iter()
                    .any(|(key, value)| string_uses_state(key) || string_uses_state(value))
        })
        || transform
            .filters
            .values()
            .any(|value| value_uses_state(value))
        || transform
            .materialize
            .as_ref()
            .is_some_and(|value| string_uses_state(value))
}

fn value_uses_state(value: &Value) -> bool {
    match value {
        Value::String(value) => string_uses_state(value),
        Value::Array(values) => values.iter().any(value_uses_state),
        Value::Object(values) => values.values().any(value_uses_state),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

fn string_uses_state(value: &str) -> bool {
    value.contains("${state")
}

fn adapter_identity(adapter: &mut dyn DatasetAdapter) -> usize {
    adapter as *mut dyn DatasetAdapter as *mut () as usize
}
