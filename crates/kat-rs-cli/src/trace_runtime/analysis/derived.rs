use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};
use serde_json::Value;

use crate::trace_runtime::{
    adapter::DatasetAdapter,
    pack::{LoadedPack, spec::TransformSpec},
    transform::run_transform,
};

pub struct DerivedRunner<'a> {
    pack: &'a LoadedPack,
    by_output_table: BTreeMap<String, &'a TransformSpec>,
    materialized: BTreeSet<String>,
}

impl<'a> DerivedRunner<'a> {
    pub fn new(pack: &'a LoadedPack) -> Self {
        let by_output_table = pack
            .transforms
            .iter()
            .map(|transform| (transform.output.table.clone(), transform))
            .collect();
        Self {
            pack,
            by_output_table,
            materialized: BTreeSet::new(),
        }
    }

    pub fn ensure_table(
        &mut self,
        adapter: &mut dyn DatasetAdapter,
        table: &str,
        params: &Value,
        state: &Value,
    ) -> Result<()> {
        let mut visiting = BTreeSet::new();
        self.ensure_table_inner(adapter, table, params, state, &mut visiting)
    }

    fn ensure_table_inner(
        &mut self,
        adapter: &mut dyn DatasetAdapter,
        table: &str,
        params: &Value,
        state: &Value,
        visiting: &mut BTreeSet<String>,
    ) -> Result<()> {
        if self.materialized.contains(table) || adapter.table_exists(table)? {
            return Ok(());
        }

        let Some(transform) = self.by_output_table.get(table).copied() else {
            bail!("table `{table}` does not exist and is not produced by a pack transform");
        };

        if !visiting.insert(table.to_string()) {
            bail!("cycle while materializing derived table `{table}`");
        }

        for input in transform.inputs.table_names() {
            if adapter.table_exists(input)? {
                continue;
            }
            if self.by_output_table.contains_key(input) {
                self.ensure_table_inner(adapter, input, params, state, visiting)?;
            } else {
                bail!(
                    "transform `{}` input table `{input}` does not exist and is not produced by a pack transform",
                    transform.id
                );
            }
        }

        run_transform(adapter, self.pack, transform, params, state)?;
        self.materialized.insert(table.to_string());
        visiting.remove(table);
        Ok(())
    }
}
