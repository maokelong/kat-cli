use std::cmp::Ordering;

use anyhow::{Result, bail};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Number, Value};

use super::binding::{BindingExpr, EvalContext};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum PredicateSpec {
    #[serde(rename = "all")]
    All(Vec<PredicateSpec>),
    #[serde(rename = "any")]
    Any(Vec<PredicateSpec>),
    #[serde(rename = "not")]
    Not(Box<PredicateSpec>),
    #[serde(rename = "eq")]
    Eq([BindingExpr; 2]),
    #[serde(rename = "neq")]
    Neq([BindingExpr; 2]),
    #[serde(rename = "gt")]
    Gt([BindingExpr; 2]),
    #[serde(rename = "gte")]
    Gte([BindingExpr; 2]),
    #[serde(rename = "lt")]
    Lt([BindingExpr; 2]),
    #[serde(rename = "lte")]
    Lte([BindingExpr; 2]),
    #[serde(rename = "exists")]
    Exists(BindingExpr),
    #[serde(rename = "temporal.pointWithin")]
    TemporalPointWithin(TemporalPointWithinSpec),
    #[serde(rename = "temporal.overlaps")]
    TemporalOverlaps(TemporalOverlapsSpec),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemporalPointWithinSpec {
    pub point: BindingExpr,
    pub window: TemporalWindowSpec,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemporalOverlapsSpec {
    pub left: TemporalWindowSpec,
    pub right: TemporalWindowSpec,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemporalWindowSpec {
    pub start: BindingExpr,
    pub end: BindingExpr,
}

impl PredicateSpec {
    pub fn matches(&self, ctx: &EvalContext<'_>) -> Result<bool> {
        match self {
            Self::All(predicates) => predicates.iter().try_fold(true, |matched, predicate| {
                Ok(matched && predicate.matches(ctx)?)
            }),
            Self::Any(predicates) => {
                for predicate in predicates {
                    if predicate.matches(ctx)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Self::Not(predicate) => Ok(!predicate.matches(ctx)?),
            Self::Eq(values) => compare_resolved(values, ctx, values_equal),
            Self::Neq(values) => {
                compare_resolved(values, ctx, |left, right| !values_equal(left, right))
            }
            Self::Gt(values) => compare_numeric(values, ctx, |ordering| ordering.is_gt()),
            Self::Gte(values) => compare_numeric(values, ctx, |ordering| !ordering.is_lt()),
            Self::Lt(values) => compare_numeric(values, ctx, |ordering| ordering.is_lt()),
            Self::Lte(values) => compare_numeric(values, ctx, |ordering| !ordering.is_gt()),
            Self::Exists(expr) => Ok(expr
                .resolve(ctx)?
                .map(|value| !value.is_null())
                .unwrap_or(false)),
            Self::TemporalPointWithin(spec) => {
                let Some(point) = resolve_number(&spec.point, ctx)? else {
                    return Ok(false);
                };
                let Some((start, end)) = resolve_window(&spec.window, ctx)? else {
                    return Ok(false);
                };
                ensure_valid_window("temporal.pointWithin window", &start, &end)?;

                Ok(
                    compare_numbers(&point, &start).is_some_and(|ordering| !ordering.is_lt())
                        && compare_numbers(&point, &end).is_some_and(|ordering| !ordering.is_gt()),
                )
            }
            Self::TemporalOverlaps(spec) => {
                let Some((left_start, left_end)) = resolve_window(&spec.left, ctx)? else {
                    return Ok(false);
                };
                let Some((right_start, right_end)) = resolve_window(&spec.right, ctx)? else {
                    return Ok(false);
                };
                ensure_valid_window("temporal.overlaps left window", &left_start, &left_end)?;
                ensure_valid_window("temporal.overlaps right window", &right_start, &right_end)?;

                Ok(compare_numbers(&left_start, &right_end)
                    .is_some_and(|ordering| ordering.is_lt())
                    && compare_numbers(&right_start, &left_end)
                        .is_some_and(|ordering| ordering.is_lt()))
            }
        }
    }
}

impl<'de> Deserialize<'de> for PredicateSpec {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let object = value.as_object().ok_or_else(|| {
            serde::de::Error::custom("graph predicate must be an object with one predicate key")
        })?;
        if object.len() != 1 {
            return Err(serde::de::Error::custom(
                "graph predicate object must contain exactly one predicate key",
            ));
        }

        let (key, raw) = object.iter().next().expect("object has exactly one entry");
        match key.as_str() {
            "all" => deserialize_variant(raw, Self::All),
            "any" => deserialize_variant(raw, Self::Any),
            "not" => deserialize_variant(raw, |predicate| Self::Not(Box::new(predicate))),
            "eq" => deserialize_variant(raw, Self::Eq),
            "neq" => deserialize_variant(raw, Self::Neq),
            "gt" => deserialize_variant(raw, Self::Gt),
            "gte" => deserialize_variant(raw, Self::Gte),
            "lt" => deserialize_variant(raw, Self::Lt),
            "lte" => deserialize_variant(raw, Self::Lte),
            "exists" => deserialize_variant(raw, Self::Exists),
            "temporal.pointWithin" => deserialize_variant(raw, Self::TemporalPointWithin),
            "temporal.overlaps" => deserialize_variant(raw, Self::TemporalOverlaps),
            other => Err(serde::de::Error::custom(format!(
                "unknown graph predicate key '{other}'"
            ))),
        }
    }
}

fn deserialize_variant<'de, T, F, E>(raw: &Value, build: F) -> std::result::Result<PredicateSpec, E>
where
    T: Deserialize<'de>,
    F: FnOnce(T) -> PredicateSpec,
    E: serde::de::Error,
{
    let value = T::deserialize(raw.clone()).map_err(E::custom)?;
    Ok(build(value))
}

fn compare_resolved(
    values: &[BindingExpr; 2],
    ctx: &EvalContext<'_>,
    compare: impl FnOnce(&Value, &Value) -> bool,
) -> Result<bool> {
    let Some(left) = values[0].resolve(ctx)? else {
        return Ok(false);
    };
    let Some(right) = values[1].resolve(ctx)? else {
        return Ok(false);
    };

    Ok(compare(&left, &right))
}

fn compare_numeric(
    values: &[BindingExpr; 2],
    ctx: &EvalContext<'_>,
    compare: impl FnOnce(Ordering) -> bool,
) -> Result<bool> {
    let Some(left) = resolve_number(&values[0], ctx)? else {
        return Ok(false);
    };
    let Some(right) = resolve_number(&values[1], ctx)? else {
        return Ok(false);
    };

    Ok(compare_numbers(&left, &right).map(compare).unwrap_or(false))
}

fn resolve_number(expr: &BindingExpr, ctx: &EvalContext<'_>) -> Result<Option<Number>> {
    Ok(expr.resolve(ctx)?.and_then(|value| match value {
        Value::Number(number) => Some(number),
        _ => None,
    }))
}

fn resolve_window(
    window: &TemporalWindowSpec,
    ctx: &EvalContext<'_>,
) -> Result<Option<(Number, Number)>> {
    let Some(start) = resolve_number(&window.start, ctx)? else {
        return Ok(None);
    };
    let Some(end) = resolve_number(&window.end, ctx)? else {
        return Ok(None);
    };

    Ok(Some((start, end)))
}

fn ensure_valid_window(context: &str, start: &Number, end: &Number) -> Result<()> {
    if compare_numbers(end, start).is_some_and(|ordering| ordering.is_lt()) {
        bail!("{context} end must be greater than or equal to start");
    }

    Ok(())
}

fn values_equal(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Number(actual), Value::Number(expected)) => numbers_equal(actual, expected),
        _ => actual == expected,
    }
}

fn numbers_equal(actual: &Number, expected: &Number) -> bool {
    match (number_kind(actual), number_kind(expected)) {
        (Some(NumericValue::Integer(actual)), Some(NumericValue::Integer(expected))) => {
            actual == expected
        }
        (Some(NumericValue::Float(actual)), Some(NumericValue::Float(expected))) => {
            actual == expected
        }
        (Some(NumericValue::Integer(actual)), Some(NumericValue::Float(expected))) => {
            float_integer_as_i128(expected).is_some_and(|expected| actual == expected)
        }
        (Some(NumericValue::Float(actual)), Some(NumericValue::Integer(expected))) => {
            float_integer_as_i128(actual).is_some_and(|actual| actual == expected)
        }
        _ => false,
    }
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
    if let Some(value) = json_integer_as_i128(number) {
        return Some(NumericValue::Integer(value));
    }

    number
        .as_f64()
        .filter(|value| value.is_finite())
        .map(NumericValue::Float)
}

fn json_integer_as_i128(number: &Number) -> Option<i128> {
    number
        .as_i64()
        .map(i128::from)
        .or_else(|| number.as_u64().map(i128::from))
}

fn compare_integer_to_float(integer: i128, float: f64) -> Option<Ordering> {
    if let Some(float_integer) = float_integer_as_i128(float) {
        return Some(integer.cmp(&float_integer));
    }

    if !float.is_finite() || float.fract() == 0.0 {
        return None;
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
