use anyhow::{Result, bail};
use serde_json::Value;

use crate::trace_runtime::adapter::{
    DatasetAdapter,
    sqlite::{SQLiteDatasetAdapter, sql::quote_identifier},
};

pub(in crate::trace_runtime::analysis) fn select_table_rows(
    adapter: &mut SQLiteDatasetAdapter,
    table: &str,
    limit: usize,
) -> Result<Vec<Value>> {
    if limit == 0 {
        bail!("select_table_rows limit must be greater than zero");
    }

    let columns = adapter.table_columns(table)?;
    if columns.is_empty() {
        bail!("table has no columns: {table}");
    }

    let column_names = columns
        .iter()
        .map(|column| column.name.clone())
        .collect::<Vec<_>>();
    let sql = select_table_rows_sql(table, &column_names, limit)?;
    adapter.query_json_rows(&sql)
}

fn select_table_rows_sql(table: &str, columns: &[String], limit: usize) -> Result<String> {
    let quoted_columns = columns
        .iter()
        .map(|column| quote_identifier(column))
        .collect::<Result<Vec<_>>>()?;
    let table = quote_identifier(table)?;
    Ok(format!(
        "SELECT {} FROM {table} LIMIT {limit}",
        quoted_columns.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use rusqlite::Connection;
    use serde_json::json;
    use tempfile::tempdir;

    use crate::trace_runtime::adapter::sqlite::SQLiteDatasetAdapter;

    use super::{select_table_rows, select_table_rows_sql};

    fn create_raw_db(path: &Path) {
        let conn = Connection::open(path).expect("raw sqlite db opens");
        conn.execute_batch(
            r#"
            CREATE TABLE "group" (
                id INTEGER,
                "select" TEXT
            );
            INSERT INTO "group" (id, "select") VALUES (1, 'one');
            INSERT INTO "group" (id, "select") VALUES (2, 'two');
            INSERT INTO "group" (id, "select") VALUES (3, 'three');
            "#,
        )
        .expect("raw schema is created");
    }

    #[test]
    fn select_table_rows_quotes_explicit_columns_in_sql() {
        let sql = select_table_rows_sql("group", &["id".to_string(), "select".to_string()], 2)
            .expect("sql is built");

        assert_eq!(sql, r#"SELECT "id", "select" FROM "group" LIMIT 2"#);
    }

    #[test]
    fn select_table_rows_quotes_identifiers_and_applies_limit() {
        let dir = tempdir().expect("tempdir");
        let raw_db = dir.path().join("raw.db");
        let scratch_db = dir.path().join("scratch.db");
        create_raw_db(&raw_db);
        let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter opens");

        let rows = select_table_rows(&mut adapter, "group", 2).expect("rows are selected");

        assert_eq!(
            rows,
            vec![
                json!({ "id": 1, "select": "one" }),
                json!({ "id": 2, "select": "two" })
            ]
        );
    }

    #[test]
    fn select_table_rows_rejects_zero_limit() {
        let dir = tempdir().expect("tempdir");
        let raw_db = dir.path().join("raw.db");
        let scratch_db = dir.path().join("scratch.db");
        create_raw_db(&raw_db);
        let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter opens");

        let error =
            select_table_rows(&mut adapter, "group", 0).expect_err("zero limit is rejected");
        let message = error.to_string();

        assert!(
            message.contains("limit") || message.contains("greater than zero"),
            "unexpected error: {message}"
        );
    }
}
