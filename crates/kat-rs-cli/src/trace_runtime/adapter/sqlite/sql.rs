use anyhow::{Result, bail};
use serde_json::Value;

pub fn quote_qualified(schema: &str, table: &str) -> Result<String> {
    Ok(format!(
        "{}.{}",
        quote_identifier(schema)?,
        quote_identifier(table)?
    ))
}

pub fn quote_identifier(identifier: &str) -> Result<String> {
    if identifier.is_empty()
        || !identifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        bail!("unsafe sqlite identifier: {identifier}");
    }
    Ok(format!("\"{identifier}\""))
}

pub fn string_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub fn scalar_literal(value: Option<&Value>) -> Result<String> {
    match value {
        None | Some(Value::Null) => Ok("NULL".to_string()),
        Some(Value::Bool(value)) => Ok(i32::from(*value).to_string()),
        Some(Value::Number(value)) => Ok(value.to_string()),
        Some(Value::String(value)) => Ok(string_literal(value)),
        Some(Value::Array(_)) | Some(Value::Object(_)) => {
            bail!("SQL placeholders only support scalar values")
        }
    }
}

pub fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\'', "''")
        .replace('%', "\\%")
        .replace('_', "\\_")
}
