use std::{cmp::Ordering, collections::HashSet};

use anyhow::Result;
use serde_json::{Number, Value};

use super::{
    GraphCandidate,
    binding::{BindingExpr, EvalContext},
    spec::GraphSelectSpec,
};

pub fn select_candidates(
    mut candidates: Vec<GraphCandidate>,
    spec: &GraphSelectSpec,
    facts: &Value,
    state: &Value,
    params: &Value,
) -> Result<Vec<GraphCandidate>> {
    let order_keys = candidates
        .iter()
        .map(|candidate| resolve_order_keys(candidate, spec, facts, state, params))
        .collect::<Result<Vec<_>>>()?;

    let mut indexed = candidates
        .drain(..)
        .zip(order_keys)
        .collect::<Vec<(GraphCandidate, Vec<Option<Value>>)>>();

    indexed.sort_by(|(_, left), (_, right)| compare_order_keys(left, right, spec));

    let mut selected = indexed
        .into_iter()
        .map(|(candidate, _)| candidate)
        .collect::<Vec<_>>();

    if !spec.dedupe_by.is_empty() {
        selected = dedupe_candidates(selected, &spec.dedupe_by, facts, state, params)?;
    }

    if let Some(limit) = spec.limit {
        selected.truncate(limit);
    }

    Ok(selected)
}

fn resolve_order_keys(
    candidate: &GraphCandidate,
    spec: &GraphSelectSpec,
    facts: &Value,
    state: &Value,
    params: &Value,
) -> Result<Vec<Option<Value>>> {
    let ctx = candidate_context(candidate, facts, state, params);
    spec.order_by
        .iter()
        .map(|order| order.expr.resolve(&ctx))
        .collect()
}

fn compare_order_keys(
    left: &[Option<Value>],
    right: &[Option<Value>],
    spec: &GraphSelectSpec,
) -> Ordering {
    for ((left, right), order) in left.iter().zip(right).zip(&spec.order_by) {
        let ordering = compare_optional_values(left, right);
        if !ordering.is_eq() {
            return if left.is_some() && right.is_some() && order.desc {
                ordering.reverse()
            } else {
                ordering
            };
        }
    }

    Ordering::Equal
}

fn compare_optional_values(left: &Option<Value>, right: &Option<Value>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => compare_values(left, right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn compare_values(left: &Value, right: &Value) -> Ordering {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => compare_numbers(left, right)
            .unwrap_or_else(|| stable_value_key(left).cmp(&stable_value_key(right))),
        (Value::String(left), Value::String(right)) => left.cmp(right),
        (Value::Bool(left), Value::Bool(right)) => left.cmp(right),
        (Value::Null, Value::Null) => Ordering::Equal,
        _ => value_rank(left)
            .cmp(&value_rank(right))
            .then_with(|| stable_value_key(left).cmp(&stable_value_key(right))),
    }
}

fn dedupe_candidates(
    candidates: Vec<GraphCandidate>,
    dedupe_by: &[BindingExpr],
    facts: &Value,
    state: &Value,
    params: &Value,
) -> Result<Vec<GraphCandidate>> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();

    for candidate in candidates {
        let ctx = candidate_context(&candidate, facts, state, params);
        let key = dedupe_by
            .iter()
            .map(|expr| expr.resolve(&ctx).map(|value| value.unwrap_or(Value::Null)))
            .collect::<Result<Vec<_>>>()?;
        let key = serde_json::to_string(&key)?;
        if seen.insert(key) {
            deduped.push(candidate);
        }
    }

    Ok(deduped)
}

fn candidate_context<'a>(
    candidate: &'a GraphCandidate,
    facts: &'a Value,
    state: &'a Value,
    params: &'a Value,
) -> EvalContext<'a> {
    EvalContext {
        source: &candidate.source,
        row: &candidate.row,
        facts,
        state,
        params,
        node: Some(&candidate.node),
    }
}

fn value_rank(value: &Value) -> u8 {
    match value {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::Number(_) => 2,
        Value::String(_) => 3,
        Value::Array(_) => 4,
        Value::Object(_) => 5,
    }
}

fn stable_value_key(value: &impl serde::Serialize) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn compare_numbers(left: &Number, right: &Number) -> Option<Ordering> {
    match (number_kind(left)?, number_kind(right)?) {
        (NumericValue::Integer(left), NumericValue::Integer(right)) => Some(left.cmp(&right)),
        (NumericValue::Float(left), NumericValue::Float(right)) => left.partial_cmp(&right),
        (NumericValue::Integer(left), NumericValue::Float(right)) => {
            compare_integer_to_float(left, right)
        }
        (NumericValue::Float(left), NumericValue::Integer(right)) => {
            compare_integer_to_float(right, left).map(Ordering::reverse)
        }
    }
}

enum NumericValue {
    Integer(i128),
    Float(f64),
}

fn number_kind(number: &Number) -> Option<NumericValue> {
    number
        .as_i64()
        .map(i128::from)
        .or_else(|| number.as_u64().map(i128::from))
        .map(NumericValue::Integer)
        .or_else(|| {
            number
                .as_f64()
                .filter(|value| value.is_finite())
                .map(NumericValue::Float)
        })
}

fn compare_integer_to_float(integer: i128, float: f64) -> Option<Ordering> {
    if let Some(float_integer) = float_integer_as_i128(float) {
        return Some(integer.cmp(&float_integer));
    }
    if !float.is_finite() {
        return None;
    }
    if float >= i128::MAX as f64 {
        return Some(Ordering::Less);
    }
    if float < i128::MIN as f64 {
        return Some(Ordering::Greater);
    }

    let floor = float_integer_as_i128(float.floor())?;
    let ceil = float_integer_as_i128(float.ceil())?;
    if integer <= floor {
        Some(Ordering::Less)
    } else if integer >= ceil {
        Some(Ordering::Greater)
    } else {
        None
    }
}

fn float_integer_as_i128(value: f64) -> Option<i128> {
    if !value.is_finite() || value.fract() != 0.0 {
        return None;
    }

    let parsed = format!("{value:.0}").parse::<i128>().ok()?;
    ((parsed as f64) == value).then_some(parsed)
}
