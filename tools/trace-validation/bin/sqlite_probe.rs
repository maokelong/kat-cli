use anyhow::Result;
use rusqlite::Connection;

fn main() -> Result<()> {
    let db = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/cpp_htrace_pbreader.db".to_string());
    let conn = Connection::open(db)?;
    let tables = [
        "args",
        "data_dict",
        "callstack",
        "js_heap_files",
        "js_heap_info",
        "js_heap_nodes",
        "js_heap_edges",
        "js_heap_string",
        "js_heap_location",
        "js_heap_sample",
        "js_heap_trace_function_info",
        "js_heap_trace_node",
        "process_measure",
        "process_measure_filter",
        "sys_mem_measure",
        "live_process",
    ];
    for table in tables {
        println!("== {table}");
        let columns = columns(&conn, table)?;
        println!("columns: {}", columns.join(", "));
        let rows: u64 =
            conn.query_row(&format!("SELECT COUNT(*) FROM \"{table}\""), [], |row| {
                row.get(0)
            })?;
        println!("rows: {rows}");
        let sql = format!("SELECT * FROM \"{table}\" LIMIT 3");
        let mut stmt = conn.prepare(&sql)?;
        let names = stmt
            .column_names()
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let mut parts = Vec::new();
            for (index, name) in names.iter().enumerate() {
                let value = row.get_ref(index)?;
                parts.push(format!("{name}={}", sqlite_value(value)));
            }
            println!("  {}", parts.join(", "));
        }
    }
    Ok(())
}

fn columns(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info(\"{table}\")"))?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(columns)
}

fn sqlite_value(value: rusqlite::types::ValueRef<'_>) -> String {
    match value {
        rusqlite::types::ValueRef::Null => "NULL".to_string(),
        rusqlite::types::ValueRef::Integer(value) => value.to_string(),
        rusqlite::types::ValueRef::Real(value) => value.to_string(),
        rusqlite::types::ValueRef::Text(value) => String::from_utf8_lossy(value).to_string(),
        rusqlite::types::ValueRef::Blob(value) => format!("<blob:{}>", value.len()),
    }
}
