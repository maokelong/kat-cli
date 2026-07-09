# Task 1 Report: SQLite Datasource Contract

## Status

DONE

## Scope Delivered

- Added workspace and crate-level `rusqlite` dependency wiring with bundled SQLite.
- Added `materialize_sqlite_dataset` export in `kat-rs-datasource`.
- Implemented SQLite table discovery, schema mapping, batch streaming, and SQLite-to-Arrow value conversion in `crates/kat-rs-datasource/src/formats/sqlite.rs`.
- Wired SQLite materialization through existing `DatasetWriter` flow without touching daemon, REST, OpenAPI, `QueryRegistry`, or `QueryResult`.
- Added datasource contract coverage for:
  - dataset remains queryable after source `.db` removal
  - empty SQLite tables materialize to queryable empty Parquet tables
  - null / real / text / blob values preserve expected query semantics

## TDD Evidence

### Red

Command:

```powershell
cargo test -p kat-rs-datasource --test sqlite_dataset_contract -- --nocapture
```

Observed failure before implementation:

```text
error[E0432]: unresolved import `kat_rs_datasource::materialize_sqlite_dataset`
```

### Green

Focused contract verification:

```powershell
cargo test -p kat-rs-datasource --test sqlite_dataset_contract -- --nocapture
```

Result:

```text
running 2 tests
test sqlite_dataset_preserves_null_real_text_and_blob_values ... ok
test sqlite_dataset_materializes_tables_and_queries_after_source_is_removed ... ok

test result: ok. 2 passed; 0 failed
```

Broader datasource verification:

```powershell
cargo test -p kat-rs-datasource
```

Result:

```text
All kat-rs-datasource tests passed, including dataset, hitrace, langfuse, proto, serde_arrow, and sqlite contracts.
```

## Files Changed

- `Cargo.toml`
- `Cargo.lock`
- `crates/kat-rs-datasource/Cargo.toml`
- `crates/kat-rs-datasource/src/formats/mod.rs`
- `crates/kat-rs-datasource/src/formats/sqlite.rs`
- `crates/kat-rs-datasource/src/materializer.rs`
- `crates/kat-rs-datasource/src/lib.rs`
- `crates/kat-rs-datasource/tests/sqlite_dataset_contract.rs`

## Commit

- `c1b1c2a feat: materialize sqlite datasets`

## Concerns

- Local `cargo add` did not support the brief's `--workspace` flag, so the workspace dependency entry was added manually to match the required end state before verification.

---

## Fix Report: SQLite source open mode and missing-file regression

Reviewed feedback pointed out that `Connection::open(path)` would create a missing SQLite file and could let `materialize_sqlite_dataset` succeed against an empty database. I changed the SQLite opener in `crates/kat-rs-datasource/src/formats/sqlite.rs` to use `Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)`, which rejects missing files and keeps the source strictly read-only.

I added a focused regression test in `crates/kat-rs-datasource/tests/sqlite_dataset_contract.rs` that calls `materialize_sqlite_dataset` with a missing source DB, asserts the call fails, and verifies the source DB path was not created.

### Verification

`cargo test -p kat-rs-datasource --test sqlite_dataset_contract -- --nocapture`

Result:

```text
running 3 tests
test sqlite_dataset_rejects_missing_source_database_without_creating_it ... ok
test sqlite_dataset_preserves_null_real_text_and_blob_values ... ok
test sqlite_dataset_materializes_tables_and_queries_after_source_is_removed ... ok

test result: ok. 3 passed; 0 failed
```

`cargo test -p kat-rs-datasource`

Result:

```text
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
...
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```
