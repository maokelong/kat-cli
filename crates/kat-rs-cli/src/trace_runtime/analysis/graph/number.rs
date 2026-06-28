use std::cmp::Ordering;

use serde_json::Number;

pub fn numbers_equal(actual: &Number, expected: &Number) -> bool {
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

pub fn compare_numbers(left: &Number, right: &Number) -> Option<Ordering> {
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

    if !float.is_finite() {
        return None;
    }
    if float >= i128::MAX as f64 {
        return Some(Ordering::Less);
    }
    if float < i128::MIN as f64 {
        return Some(Ordering::Greater);
    }
    if float.fract() == 0.0 {
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
