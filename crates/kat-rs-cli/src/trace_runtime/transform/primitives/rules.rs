use anyhow::Result;

use crate::trace_runtime::adapter::{
    DatasetAdapter,
    sqlite::sql::{escape_like, quote_identifier, string_literal},
};

pub struct ClassifyRuleSet {
    pub source_table: String,
    pub output_table: String,
    pub id_column: String,
    pub text_column: String,
    pub rules: Vec<(String, Vec<String>, Vec<String>)>,
}

pub fn run_rules_classify(adapter: &mut dyn DatasetAdapter, rules: &ClassifyRuleSet) -> Result<()> {
    let text_column = quote_identifier(&rules.text_column)?;
    let mut cases = Vec::new();
    for (class, includes, excludes) in &rules.rules {
        let include_expr = includes
            .iter()
            .map(|needle| {
                format!(
                    "LOWER({text_column}) LIKE '%{}%' ESCAPE '\\'",
                    escape_like(&needle.to_ascii_lowercase())
                )
            })
            .collect::<Vec<_>>()
            .join(" OR ");
        let exclude_expr = excludes
            .iter()
            .map(|needle| {
                format!(
                    "LOWER({text_column}) NOT LIKE '%{}%' ESCAPE '\\'",
                    escape_like(&needle.to_ascii_lowercase())
                )
            })
            .collect::<Vec<_>>()
            .join(" AND ");
        let condition = match (include_expr.is_empty(), exclude_expr.is_empty()) {
            (false, false) => format!("({include_expr}) AND ({exclude_expr})"),
            (false, true) => format!("({include_expr})"),
            (true, false) => format!("({exclude_expr})"),
            (true, true) => "0".to_string(),
        };
        cases.push(format!("WHEN {condition} THEN {}", string_literal(class)));
    }

    let class_expr = if cases.is_empty() {
        string_literal("unclassified")
    } else {
        format!(
            "CASE {} ELSE {} END",
            cases.join(" "),
            string_literal("unclassified")
        )
    };

    let sql = format!(
        "SELECT {id} AS itid, {text} AS thread_name, {class_expr} AS class FROM {source}",
        id = quote_identifier(&rules.id_column)?,
        text = text_column,
        source = quote_identifier(&rules.source_table)?,
    );
    adapter.create_derived_table_as(&rules.output_table, &sql)
}
