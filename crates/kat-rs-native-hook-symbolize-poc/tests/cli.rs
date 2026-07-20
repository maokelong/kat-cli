use std::{path::Path, process::Command};

use calamine::{Data, Reader, Xlsx, open_workbook};
use rusqlite::Connection;

fn create_database(path: &Path, with_rows: bool) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE data_dict (id INTEGER PRIMARY KEY, data TEXT);
             CREATE TABLE native_hook_frame (
                 id INTEGER PRIMARY KEY,
                 callchain_id INTEGER NOT NULL,
                 depth INTEGER NOT NULL,
                 symbol_id INTEGER,
                 file_id INTEGER,
                 vaddr INTEGER NOT NULL
             );",
        )
        .unwrap();
    if !with_rows {
        return;
    }
    connection
        .execute_batch(
            "INSERT INTO data_dict (id, data) VALUES
                 (1, '/system/lib/libsymbol.so+0x2'),
                 (2, '/system/lib/libfallback.so'),
                 (3, '');
             INSERT INTO native_hook_frame
                 (id, callchain_id, depth, symbol_id, file_id, vaddr) VALUES
                 (3, 2, 0, 1, NULL, 99),
                 (2, 1, 2, 3, 2, 16),
                 (1, 1, 1, 1, NULL, 88);",
        )
        .unwrap();
}

fn run_cli(database: &Path, symbol_dir: &Path, output: &Path) {
    let result = Command::new(env!("CARGO_BIN_EXE_kat-native-hook-symbolize"))
        .arg(database)
        .arg("--symbol-dir")
        .arg(symbol_dir)
        .arg("--output")
        .arg(output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn text(value: &Data) -> &str {
    match value {
        Data::String(value) => value,
        other => panic!("expected string cell, got {other:?}"),
    }
}

#[test]
fn exports_stably_sorted_frames_and_missing_modules_and_overwrites_output() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("trace.db");
    let symbols = directory.path().join("symbols");
    let output = directory.path().join("symbols.xlsx");
    std::fs::create_dir(&symbols).unwrap();
    create_database(&database, true);

    std::fs::write(&output, "old workbook").unwrap();
    run_cli(&database, &symbols, &output);

    let mut workbook: Xlsx<_> = open_workbook(&output).unwrap();
    assert_eq!(workbook.sheet_names(), ["symbols", "missing_modules"]);
    let symbol_rows = workbook
        .worksheet_range("symbols")
        .unwrap()
        .rows()
        .map(|row| row.iter().map(ToString::to_string).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    assert_eq!(
        symbol_rows,
        vec![
            vec![
                "callchain_id",
                "depth",
                "original_symbol",
                "resolved_symbol"
            ],
            vec![
                "1",
                "1",
                "/system/lib/libsymbol.so+0x2",
                "/system/lib/libsymbol.so+0x2"
            ],
            vec![
                "1",
                "2",
                "/system/lib/libfallback.so+0x10",
                "/system/lib/libfallback.so+0x10"
            ],
            vec![
                "2",
                "0",
                "/system/lib/libsymbol.so+0x2",
                "/system/lib/libsymbol.so+0x2"
            ],
        ]
    );
    let missing = workbook.worksheet_range("missing_modules").unwrap();
    let rows = missing.rows().collect::<Vec<_>>();
    assert_eq!(text(&rows[0][0]), "module_path");
    assert_eq!(text(&rows[0][1]), "occurrence_count");
    assert_eq!(text(&rows[1][0]), "/system/lib/libfallback.so");
    assert_eq!(rows[1][1], Data::Float(1.0));
    assert_eq!(text(&rows[2][0]), "/system/lib/libsymbol.so");
    assert_eq!(rows[2][1], Data::Float(2.0));
}

#[test]
fn exports_header_only_workbook_for_empty_frame_table() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("trace.db");
    let symbols = directory.path().join("symbols");
    let output = directory.path().join("symbols.xlsx");
    std::fs::create_dir(&symbols).unwrap();
    create_database(&database, false);

    run_cli(&database, &symbols, &output);

    let mut workbook: Xlsx<_> = open_workbook(&output).unwrap();
    assert_eq!(workbook.worksheet_range("symbols").unwrap().height(), 1);
    assert_eq!(
        workbook
            .worksheet_range("missing_modules")
            .unwrap()
            .height(),
        1
    );
}
