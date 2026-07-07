use std::collections::BTreeMap;

use serde_json::Value;

use crate::error::ApiError;

pub fn render_sql(sql: &str, inputs: &BTreeMap<String, Value>) -> Result<String, ApiError> {
    let mut rendered = sql.to_string();
    for (name, value) in inputs {
        let marker = format!("{{{{inputs.{name}}}}}");
        if rendered.contains(&marker) {
            rendered = rendered.replace(&marker, &sql_literal(value)?);
        }
    }
    if let Some(start) = rendered.find("{{inputs.") {
        let end = rendered[start..]
            .find("}}")
            .map(|offset| start + offset + 2)
            .unwrap_or(rendered.len());
        return Err(ApiError::validation(format!(
            "unresolved SQL template input: {}",
            &rendered[start..end]
        )));
    }
    Ok(rendered)
}

fn sql_literal(value: &Value) -> Result<String, ApiError> {
    match value {
        Value::String(value) => Ok(value.replace('\'', "''")),
        Value::Number(value) => Ok(value.to_string()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Null => Err(ApiError::validation(
            "null SQL template input is not supported",
        )),
        Value::Array(_) | Value::Object(_) => Err(ApiError::validation(
            "array/object SQL template input is not supported",
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::render_sql;

    #[test]
    fn render_sql_replaces_markers_and_escapes_string_literals() {
        let inputs = BTreeMap::from([
            ("pattern".to_string(), json!("o'hara")),
            ("limit".to_string(), json!(8)),
            ("enabled".to_string(), json!(true)),
        ]);

        let rendered = render_sql(
            "select '{{inputs.pattern}}', {{inputs.limit}}, {{inputs.enabled}}",
            &inputs,
        )
        .expect("sql renders");

        assert_eq!(rendered, "select 'o''hara', 8, true");
    }

    #[test]
    fn render_sql_rejects_unresolved_markers() {
        let error = render_sql("select '{{inputs.missing}}'", &BTreeMap::new())
            .expect_err("missing marker should fail");

        assert_eq!(
            error.message,
            "unresolved SQL template input: {{inputs.missing}}"
        );
    }
}
