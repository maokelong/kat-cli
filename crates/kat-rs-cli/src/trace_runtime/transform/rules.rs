use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::trace_runtime::{
    adapter::DatasetAdapter,
    pack::{LoadedPack, spec::TransformSpec},
    transform::primitives::rules::{ClassifyRuleSet, run_rules_classify},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleConfig {
    field: String,
    #[serde(default)]
    contains: Option<String>,
    #[serde(default)]
    any: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    exclude: Vec<String>,
}

pub fn run_rules_classify_transform(
    adapter: &mut dyn DatasetAdapter,
    pack: &LoadedPack,
    transform: &TransformSpec,
) -> Result<()> {
    if transform.kind != "rules.classify" {
        bail!("transform `{}` is not rules.classify", transform.id);
    }
    super::reject_marker_only_config(transform, "rules.classify")?;

    let input_tables = transform.inputs.table_names();
    let [source_table] = input_tables.as_slice() else {
        bail!(
            "rules.classify transform `{}` requires exactly one input table",
            transform.id
        );
    };

    if transform.safety.allowed_tables.is_empty() {
        bail!(
            "rules.classify transform `{}` requires non-empty safety.allowedTables",
            transform.id
        );
    }
    if !transform
        .safety
        .allowed_tables
        .iter()
        .any(|table| table == source_table)
    {
        bail!(
            "rules.classify transform `{}` source_table `{}` is outside safety.allowedTables: {}",
            transform.id,
            source_table,
            transform.safety.allowed_tables.join(", ")
        );
    }

    if !adapter.table_exists(source_table)? {
        bail!(
            "rules.classify transform `{}` source_table does not exist: {}",
            transform.id,
            source_table
        );
    }

    let non_empty_rule_sets = pack
        .rule_sets
        .iter()
        .filter(|rule_set| !rule_set.rules.is_empty())
        .collect::<Vec<_>>();
    let [rule_set] = non_empty_rule_sets.as_slice() else {
        bail!(
            "rules.classify transform `{}` requires exactly one non-empty pack rule set, found {}",
            transform.id,
            non_empty_rule_sets.len()
        );
    };

    let mut text_column = None;
    let mut rules = Vec::new();
    for (class, value) in &rule_set.rules {
        let config: RuleConfig = serde_json::from_value(value.clone())
            .with_context(|| format!("failed to parse rule `{class}` for `{}`", transform.id))?;
        if let Some(existing) = &text_column {
            if existing != &config.field {
                bail!(
                    "rules.classify transform `{}` has rules with multiple fields: `{}` and `{}`",
                    transform.id,
                    existing,
                    config.field
                );
            }
        } else {
            text_column = Some(config.field.clone());
        }

        let mut includes = Vec::new();
        if let Some(contains) = config.contains {
            includes.push(contains);
        }
        includes.extend(config.any);
        rules.push((class.clone(), includes, config.exclude));
    }

    let text_column = text_column.ok_or_else(|| {
        anyhow::anyhow!(
            "rules.classify transform `{}` has no rules in selected rule set",
            transform.id
        )
    })?;
    let spec = ClassifyRuleSet {
        source_table: (*source_table).to_string(),
        output_table: transform.output.table.clone(),
        id_column: "itid".to_string(),
        text_column,
        rules,
    };

    run_rules_classify(adapter, &spec)
}

fn deserialize_string_or_vec<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec {
        String(String),
        Vec(Vec<String>),
    }

    Ok(match Option::<StringOrVec>::deserialize(deserializer)? {
        Some(StringOrVec::String(value)) => vec![value],
        Some(StringOrVec::Vec(values)) => values,
        None => Vec::new(),
    })
}
