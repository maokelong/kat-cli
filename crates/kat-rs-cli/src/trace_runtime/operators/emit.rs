use serde_json::{Value, json};

pub fn emit_rows(schema: &str, rows: Vec<Value>) -> Value {
    let status = if rows.is_empty() {
        "empty_result"
    } else {
        "ok"
    };
    let row_count = rows.len();

    json!({
        "schema": schema,
        "status": status,
        "facts": {
            "row_count": row_count,
            "rows": rows
        },
        "limitations": []
    })
}
