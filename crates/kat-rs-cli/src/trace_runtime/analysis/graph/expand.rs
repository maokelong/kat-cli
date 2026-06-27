use anyhow::{Result, bail};
use serde_json::{Map, Number, Value};

use super::{
    binding::EvalContext,
    spec::{GraphExpandSpec, GraphOutputSpec, GraphValueSpec},
};

pub fn expand_node(spec: &GraphExpandSpec, ctx: &EvalContext<'_>) -> Result<Value> {
    if let Some(same_as) = &spec.node.same_as {
        if let Some(value) = same_as.resolve(ctx)? {
            return Ok(value);
        }
    }

    let mut node = Value::Object(Map::new());
    for (path, value_spec) in &spec.node.fields {
        let Some(value) = resolve_value(value_spec, ctx)? else {
            continue;
        };
        insert_path(&mut node, path, value)?;
    }

    Ok(node)
}

pub fn output_annotations(spec: &GraphOutputSpec, ctx: &EvalContext<'_>) -> Result<Value> {
    let mut annotations = Map::new();
    for (key, value_spec) in &spec.annotations {
        let Some(value) = resolve_value(value_spec, ctx)? else {
            continue;
        };
        annotations.insert(key.clone(), value);
    }

    Ok(Value::Object(annotations))
}

pub fn resolve_value(spec: &GraphValueSpec, ctx: &EvalContext<'_>) -> Result<Option<Value>> {
    match spec {
        GraphValueSpec::Value(value) => value.resolve(ctx),
        GraphValueSpec::Scaled { value, scale } => {
            let Some(resolved) = value.resolve(ctx)? else {
                return Ok(None);
            };
            let Some(number) = resolved.as_f64() else {
                return Ok(Some(resolved));
            };
            let scaled = number * scale;
            let Some(number) = Number::from_f64(scaled) else {
                return Ok(Some(resolved));
            };
            Ok(Some(Value::Number(number)))
        }
    }
}

fn insert_path(target: &mut Value, path: &str, value: Value) -> Result<()> {
    if path.is_empty() {
        bail!("empty graph node field path");
    }

    let segments = path.split('.').collect::<Vec<_>>();
    if segments.iter().any(|segment| segment.is_empty()) {
        bail!("empty graph node field path segment in '{path}'");
    }

    let mut current = target;
    for segment in &segments[..segments.len() - 1] {
        let object = current
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("graph node field path '{path}' crosses non-object"))?;
        current = object
            .entry((*segment).to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if !current.is_object() {
            bail!("graph node field path '{path}' crosses non-object");
        }
    }

    let leaf = segments
        .last()
        .expect("field path has at least one validated segment");
    let object = current
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("graph node field path '{path}' crosses non-object"))?;
    object.insert((*leaf).to_string(), value);

    Ok(())
}
