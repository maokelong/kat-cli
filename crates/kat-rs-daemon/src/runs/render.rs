use regex::Regex;
use serde_json::Value;

use crate::error::ApiError;

use super::context::{ContextStore, ContextValue};

pub fn render_template(input: &str, context: &ContextStore) -> Result<String, ApiError> {
    let pattern = Regex::new(r"\{\{ctx\.([A-Za-z0-9_]+)(?:\.(start|end))?\}\}")
        .map_err(|error| ApiError::internal(format!("invalid context regex: {error}")))?;
    let mut rendered = String::with_capacity(input.len());
    let mut last = 0;

    for captures in pattern.captures_iter(input) {
        let whole = captures.get(0).expect("whole match exists");
        rendered.push_str(&input[last..whole.start()]);
        let slot = captures.get(1).expect("slot capture exists").as_str();
        let field = captures.get(2).map(|field| field.as_str());
        rendered.push_str(&render_value(slot, field, context)?);
        last = whole.end();
    }
    rendered.push_str(&input[last..]);

    Ok(rendered)
}

fn render_value(
    slot: &str,
    field: Option<&str>,
    context: &ContextStore,
) -> Result<String, ApiError> {
    match (context.value(slot)?, field) {
        (ContextValue::Scalar(value), None) => Ok(render_scalar(value)),
        (ContextValue::Interval { start, .. }, Some("start")) => Ok(start.to_string()),
        (ContextValue::Interval { end, .. }, Some("end")) => Ok(end.to_string()),
        (ContextValue::Scalar(_), Some(field)) => Err(ApiError::validation(format!(
            "context scalar slot {slot} does not have field {field}"
        ))),
        (ContextValue::Interval { .. }, None) => Err(ApiError::validation(format!(
            "context interval slot {slot} must reference start or end"
        ))),
        (ContextValue::Interval { .. }, Some(field)) => Err(ApiError::validation(format!(
            "context interval slot {slot} does not have field {field}"
        ))),
    }
}

fn render_scalar(value: &Value) -> String {
    match value {
        Value::Null => "NULL".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.replace('\'', "''"),
        Value::Array(_) | Value::Object(_) => value.to_string().replace('\'', "''"),
    }
}
