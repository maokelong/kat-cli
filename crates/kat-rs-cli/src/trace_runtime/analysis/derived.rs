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
    materialized: BTreeSet<String>,
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
            materialized: BTreeSet::new(),
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
        if adapter.table_exists(table)? {
            if self.materialized.contains(table) || !self.by_output_table.contains_key(table) {
                return Ok(());
            }
            let transform = self
                .by_output_table
                .get(table)
                .expect("output table index was checked");
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
                self.ensure_table_inner(adapter, input, params, state, visiting)?;
            } else if !adapter.table_exists(input)? {
                bail!(
                    "transform `{}` input table `{input}` does not exist and is not produced by a pack transform",
                    transform.id
                );
            }
        }

        run_transform(adapter, self.pack, transform, params, state)
            .with_context(|| format!("failed to run transform `{}`", transform.id))?;
        self.materialized.insert(table.to_string());
        visiting.remove(table);
        Ok(())
    }
}
