use anyhow::{Result, bail};
use serde_json::Value;

pub fn resolve_template(template: &str, params: &Value, state: &Value) -> Result<Value> {
    if let Some(path) = exact_placeholder(template) {
        return lookup_placeholder(path, params, state).cloned();
    }

    let mut rendered = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("${") {
        rendered.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find('}') else {
            bail!("unterminated binding placeholder in {template:?}");
        };
        let path = after_start[..end].trim();
        let value = lookup_placeholder(path, params, state)?;
        rendered.push_str(value_to_string(value)?);
        rest = &after_start[end + 1..];
    }
    rendered.push_str(rest);
    Ok(Value::String(rendered))
}

fn exact_placeholder(template: &str) -> Option<&str> {
    let inner = template.strip_prefix("${")?;
    let end = inner.find('}')?;
    if end == inner.len() - 1 {
        Some(inner[..end].trim())
    } else {
        None
    }
}

fn lookup_placeholder<'a>(path: &str, params: &'a Value, state: &'a Value) -> Result<&'a Value> {
    let mut parts = path.split('.');
    let Some(root) = parts.next() else {
        bail!("empty binding placeholder");
    };

    let mut value = match root {
        "params" => params,
        "state" => state,
        _ => bail!("unsupported binding root {root:?}"),
    };

    let Some(first_part) = parts.next() else {
        bail!("binding path must include a field after {root:?}");
    };
    if first_part.is_empty() {
        bail!("empty binding path segment in {path:?}");
    }
    value = value
        .get(first_part)
        .ok_or_else(|| anyhow::anyhow!("missing binding path {path:?}"))?;

    for part in parts {
        if part.is_empty() {
            bail!("empty binding path segment in {path:?}");
        }
        value = value
            .get(part)
            .ok_or_else(|| anyhow::anyhow!("missing binding path {path:?}"))?;
    }

    Ok(value)
}

fn value_to_string(value: &Value) -> Result<&str> {
    match value {
        Value::String(value) => Ok(value),
        _ => bail!("inline binding value must be a string"),
    }
}
