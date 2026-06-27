use std::cmp::Ordering;

use anyhow::{Result, bail};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Number, Value};

use super::{
    binding::{BindingExpr, EvalContext},
    number::{compare_numbers, numbers_equal},
};

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
            Self::All(predicates) => {
                for predicate in predicates {
                    if !predicate.matches(ctx)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
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
                let Some((start, end)) = resolve_window(&spec.window, ctx)? else {
                    return Ok(false);
                };
                ensure_valid_window("temporal.pointWithin window", &start, &end)?;
                let Some(point) = resolve_number(&spec.point, ctx)? else {
                    return Ok(false);
                };

                Ok(
                    compare_numbers(&point, &start).is_some_and(|ordering| !ordering.is_lt())
                        && compare_numbers(&point, &end).is_some_and(|ordering| !ordering.is_gt()),
                )
            }
            Self::TemporalOverlaps(spec) => {
                let left = resolve_window(&spec.left, ctx)?;
                let right = resolve_window(&spec.right, ctx)?;
                if let Some((left_start, left_end)) = &left {
                    ensure_valid_window("temporal.overlaps left window", left_start, left_end)?;
                }
                if let Some((right_start, right_end)) = &right {
                    ensure_valid_window("temporal.overlaps right window", right_start, right_end)?;
                }
                let (Some((left_start, left_end)), Some((right_start, right_end))) = (left, right)
                else {
                    return Ok(false);
                };

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
