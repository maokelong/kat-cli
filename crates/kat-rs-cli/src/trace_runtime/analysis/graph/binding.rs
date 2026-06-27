use anyhow::{Result, bail};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub enum BindingExpr {
    Literal(Value),
    Path(String),
    Template(String),
}

pub struct EvalContext<'a> {
    pub source: &'a Value,
    pub row: &'a Value,
    pub facts: &'a Value,
    pub state: &'a Value,
    pub params: &'a Value,
    pub node: Option<&'a Value>,
}

impl BindingExpr {
    pub fn resolve(&self, ctx: &EvalContext<'_>) -> Result<Option<Value>> {
        match self {
            Self::Literal(value) => Ok(Some(value.clone())),
            Self::Path(path) => resolve_path(path, ctx),
            Self::Template(template) => resolve_template(template, ctx),
        }
    }
}

impl Serialize for BindingExpr {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Literal(value) => value.serialize(serializer),
            Self::Path(path) | Self::Template(path) => path.serialize(serializer),
        }
    }
}

// Serialization emits the shorthand/canonical shape. The explicit
// `{ literal: ... }` form is for authors to disambiguate literal strings in
// specs, not a guaranteed round-trip output shape.
impl<'de> Deserialize<'de> for BindingExpr {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        Ok(match value {
            Value::Object(mut object) if object.contains_key("literal") => {
                if object.len() != 1 {
                    return Err(serde::de::Error::custom(
                        "explicit graph binding literal must contain only the 'literal' key",
                    ));
                }
                let literal = object.remove("literal").ok_or_else(|| {
                    serde::de::Error::custom("explicit graph binding literal missing value")
                })?;
                Self::Literal(literal)
            }
            Value::String(value) if value.contains("${") => Self::Template(value),
            Value::String(value) if is_supported_root_path(&value) => Self::Path(value),
            value => Self::Literal(value),
        })
    }
}

fn resolve_template(template: &str, ctx: &EvalContext<'_>) -> Result<Option<Value>> {
    if template.starts_with("${") {
        if let Some(path) = exact_template_path(template)? {
            return resolve_path(path, ctx);
        }
    }

    let mut rendered = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("${") {
        rendered.push_str(&rest[..start]);
        let placeholder = &rest[start + 2..];
        let Some(end) = placeholder.find('}') else {
            bail!("unterminated binding template placeholder in '{template}'");
        };
        let path = &placeholder[..end];
        validate_template_path(path, template)?;
        if let Some(value) = resolve_path(path, ctx)? {
            rendered.push_str(&render_inline_value(&value));
        }
        rest = &placeholder[end + 1..];
    }
    rendered.push_str(rest);

    Ok(Some(Value::String(rendered)))
}

fn exact_template_path(template: &str) -> Result<Option<&str>> {
    let Some(rest) = template.strip_prefix("${") else {
        return Ok(None);
    };
    let Some(end) = rest.find('}') else {
        bail!("unterminated binding template placeholder in '{template}'");
    };
    let trailing = &rest[end + 1..];
    if trailing.is_empty() {
        let path = &rest[..end];
        validate_template_path(path, template)?;
        return Ok(Some(path));
    }
    if trailing.starts_with('}') {
        bail!("unexpected trailing content after exact binding template '{template}'");
    }

    let path = &rest[..end];
    validate_template_path(path, template)?;
    Ok(None)
}

fn validate_template_path(path: &str, template: &str) -> Result<()> {
    if path.trim().is_empty() {
        bail!("empty binding template placeholder in '{template}'");
    }
    if path.contains("${") {
        bail!("nested binding template placeholder '{path}' in '{template}'");
    }
    Ok(())
}

fn render_inline_value(value: &Value) -> String {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| value.to_string())
}

fn resolve_path(path: &str, ctx: &EvalContext<'_>) -> Result<Option<Value>> {
    let mut segments = path.split('.');
    let Some(root) = segments.next() else {
        bail!("empty binding path");
    };
    if root.is_empty() {
        bail!("empty binding path segment in '{path}'");
    }

    let Some(mut current) = root_value(root, ctx)? else {
        return Ok(None);
    };

    for segment in segments {
        if segment.is_empty() {
            bail!("empty binding path segment in '{path}'");
        }
        let Some(next) = current.get(segment) else {
            return Ok(None);
        };
        current = next;
    }

    Ok(Some(current.clone()))
}

fn root_value<'a>(root: &str, ctx: &'a EvalContext<'_>) -> Result<Option<&'a Value>> {
    match root {
        "source" => Ok(Some(ctx.source)),
        "row" => Ok(Some(ctx.row)),
        "facts" => Ok(Some(ctx.facts)),
        "state" => Ok(Some(ctx.state)),
        "params" => Ok(Some(ctx.params)),
        "node" => Ok(ctx.node),
        _ => bail!("unknown binding path root '{root}'"),
    }
}

fn is_supported_root_path(value: &str) -> bool {
    supported_root(value)
        .is_some_and(|root| value.len() == root.len() || value.as_bytes()[root.len()] == b'.')
}

fn supported_root(value: &str) -> Option<&'static str> {
    ["source", "row", "facts", "state", "params", "node"]
        .into_iter()
        .find(|root| value.starts_with(root))
}
