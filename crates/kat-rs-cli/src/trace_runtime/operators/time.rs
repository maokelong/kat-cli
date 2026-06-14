use serde_json::Value;

pub fn i64_field(row: &Value, field: &str) -> Option<i64> {
    match row.get(field)? {
        Value::Number(number) => number.as_i64().or_else(|| number.as_u64()?.try_into().ok()),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

pub fn u64_field(row: &Value, field: &str) -> Option<u64> {
    match row.get(field)? {
        Value::Number(number) => number.as_u64().or_else(|| number.as_i64()?.try_into().ok()),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

pub fn bool_field(row: &Value, field: &str) -> Option<bool> {
    match row.get(field)? {
        Value::Bool(value) => Some(*value),
        Value::Number(number) => Some(number.as_i64()? != 0),
        Value::String(value) => match value.to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Some(true),
            "false" | "0" | "no" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

pub fn string_field(row: &Value, field: &str) -> Option<String> {
    match row.get(field)? {
        Value::String(value) if !value.is_empty() => Some(value.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

pub fn clipped_duration(ts: i64, dur: i64, start_ts: i64, end_ts: i64) -> Option<(i64, i64, i64)> {
    let slice_end = if dur <= 0 {
        end_ts
    } else {
        ts.checked_add(dur)?
    };
    let clip_start = ts.max(start_ts);
    let clip_end = slice_end.min(end_ts);
    (clip_end > clip_start).then_some((clip_start, clip_end, clip_end - clip_start))
}
