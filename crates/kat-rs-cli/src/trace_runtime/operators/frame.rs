use anyhow::{Result, bail};
use serde_json::{Value, json};

use super::time::{i64_field, string_field};

pub fn extract_first_draw_window(schema: &str, rows: Vec<Value>) -> Result<Value> {
    let Some(row) = rows.first() else {
        return Ok(json!({
            "schema": schema,
            "status": "empty_result",
            "facts": {
                "rows": []
            },
            "limitations": ["no firstDrawFrame marker found"]
        }));
    };

    let payload = string_field(row, "marker_payload").unwrap_or_default();
    let frame_start_ts = payload_timestamp(&payload, "layoutMeasureDurationStartTimestamp")
        .or_else(|| i64_field(row, "frame_start_ts"))
        .ok_or_else(|| anyhow::anyhow!("missing first draw start timestamp"))?;
    let frame_end_ts = payload_timestamp(&payload, "layoutMeasureDurationEndTimestamp")
        .or_else(|| i64_field(row, "frame_end_ts"))
        .ok_or_else(|| anyhow::anyhow!("missing first draw end timestamp"))?;

    if frame_end_ts < frame_start_ts {
        bail!("first draw end timestamp is earlier than start timestamp");
    }

    Ok(json!({
        "schema": schema,
        "status": "ok",
        "facts": {
            "marker_ts": i64_field(row, "marker_ts"),
            "frame_start_ts": frame_start_ts,
            "frame_end_ts": frame_end_ts,
            "duration_ns": frame_end_ts - frame_start_ts,
            "root_thread_itid": i64_field(row, "itid"),
            "root_thread_tid": i64_field(row, "tid"),
            "process_name": string_field(row, "process_name"),
            "pid": i64_field(row, "pid"),
            "marker_payload": payload
        },
        "limitations": []
    }))
}

fn payload_timestamp(payload: &str, key: &str) -> Option<i64> {
    let marker = format!("{key}:");
    let start = payload.find(&marker)? + marker.len();
    let value = payload[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();

    value.parse().ok()
}
