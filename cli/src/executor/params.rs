use crate::config::models::{Atomic, AtomicInput};
use anyhow::{bail, Result};
use std::collections::BTreeMap;

pub fn parse_params(values: &[String]) -> Result<BTreeMap<String, String>> {
    let mut params = BTreeMap::new();
    for value in values {
        let Some((key, val)) = value.split_once('=') else {
            bail!("参数必须使用 key=value 格式: {value}");
        };
        params.insert(key.to_string(), val.to_string());
    }
    Ok(params)
}

pub fn prepare_sql(atomic: &Atomic, params: &BTreeMap<String, String>) -> Result<String> {
    for (name, input) in &atomic.inputs {
        if input.required && !params.contains_key(name) {
            bail!("缺少必需参数: {name}");
        }
    }

    let mut merged = BTreeMap::new();
    for (name, input) in &atomic.inputs {
        let value = params
            .get(name)
            .cloned()
            .unwrap_or_else(|| default_value_for(input));
        merged.insert(name.clone(), value);
    }
    for (name, value) in params {
        merged.entry(name.clone()).or_insert_with(|| value.clone());
    }

    substitute_named_params(&atomic.sql, &merged, &atomic.inputs)
}

fn substitute_named_params(
    sql: &str,
    params: &BTreeMap<String, String>,
    inputs: &BTreeMap<String, AtomicInput>,
) -> Result<String> {
    let mut out = sql.to_string();
    let mut keys: Vec<_> = params.keys().collect();
    keys.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));

    for key in keys {
        let value = params.get(key).expect("key collected from params");
        let literal = sql_literal(key, value, inputs.get(key))?;
        out = out.replace(&format!(":{key}"), &literal);
    }
    Ok(out)
}

fn default_value_for(input: &AtomicInput) -> String {
    match input.type_name.as_str() {
        "bool" => "false".to_string(),
        "int64" | "timestamp" | "duration" | "duration_ns" => "0".to_string(),
        _ => String::new(),
    }
}

fn sql_literal(name: &str, value: &str, input: Option<&AtomicInput>) -> Result<String> {
    match input.map(|input| input.type_name.as_str()) {
        Some("int64" | "timestamp" | "duration" | "duration_ns") => numeric_literal(name, value),
        Some("bool") => bool_literal(name, value),
        _ => Ok(quoted_literal(value)),
    }
}

fn numeric_literal(name: &str, value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.parse::<i64>().is_err() {
        bail!("参数 {name} 必须是整数数值，实际为: {value}");
    }
    Ok(trimmed.to_string())
}

fn bool_literal(name: &str, value: &str) -> Result<String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" => Ok("1".to_string()),
        "false" | "0" => Ok("0".to_string()),
        _ => bail!("参数 {name} 必须是 bool 值，实际为: {value}"),
    }
}

fn quoted_literal(value: &str) -> String {
    let escaped = value.replace('\'', "''");
    format!("'{escaped}'")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::models::{AtomicOutputs, AtomicResources};

    fn input(type_name: &str, required: bool) -> AtomicInput {
        AtomicInput {
            type_name: type_name.to_string(),
            required,
        }
    }

    fn atomic(sql: &str, inputs: BTreeMap<String, AtomicInput>) -> Atomic {
        Atomic {
            id: "test_atomic".to_string(),
            domain: "scheduler-kernel".to_string(),
            engine: "perfetto-sql".to_string(),
            description: "test".to_string(),
            inputs,
            resources: AtomicResources {
                timeout_ms: 1000,
                max_rows: 10,
                max_result_bytes: 1024,
                priority: "p1".to_string(),
            },
            sql: sql.to_string(),
            outputs: AtomicOutputs { columns: vec![] },
        }
    }

    #[test]
    fn prepare_sql_keeps_numeric_params_unquoted_and_quotes_strings() {
        let mut inputs = BTreeMap::new();
        inputs.insert("utid".to_string(), input("int64", true));
        inputs.insert("start_ts".to_string(), input("timestamp", true));
        inputs.insert("process_name".to_string(), input("string", true));
        inputs.insert("enabled".to_string(), input("bool", true));
        let atomic = atomic(
            "SELECT * FROM x WHERE utid = :utid AND ts >= :start_ts AND name = :process_name AND enabled = :enabled;",
            inputs,
        );

        let mut params = BTreeMap::new();
        params.insert("utid".to_string(), "7".to_string());
        params.insert("start_ts".to_string(), "244820587000".to_string());
        params.insert("process_name".to_string(), "hi'view".to_string());
        params.insert("enabled".to_string(), "true".to_string());

        let sql = prepare_sql(&atomic, &params).unwrap();
        assert!(sql.contains("utid = 7"));
        assert!(sql.contains("ts >= 244820587000"));
        assert!(sql.contains("name = 'hi''view'"));
        assert!(sql.contains("enabled = 1"));
    }

    #[test]
    fn prepare_sql_rejects_non_numeric_for_numeric_input() {
        let mut inputs = BTreeMap::new();
        inputs.insert("utid".to_string(), input("int64", true));
        let atomic = atomic("SELECT * FROM x WHERE utid = :utid;", inputs);

        let mut params = BTreeMap::new();
        params.insert("utid".to_string(), "7 OR 1=1".to_string());

        let error = prepare_sql(&atomic, &params).unwrap_err().to_string();
        assert!(error.contains("参数 utid 必须是整数数值"));
    }

    #[test]
    fn prepare_sql_replaces_longer_param_names_first() {
        let mut inputs = BTreeMap::new();
        inputs.insert("id".to_string(), input("int64", true));
        inputs.insert("id2".to_string(), input("int64", true));
        let atomic = atomic("SELECT :id2 AS a, :id AS b;", inputs);

        let mut params = BTreeMap::new();
        params.insert("id".to_string(), "1".to_string());
        params.insert("id2".to_string(), "2".to_string());

        let sql = prepare_sql(&atomic, &params).unwrap();
        assert_eq!(sql, "SELECT 2 AS a, 1 AS b;");
    }
}
