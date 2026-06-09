# kat-rs datasource / session / cli Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the minimal kat-rs datasource/session/cli architecture for protobuf hitrace SQL queries.

**Architecture:** The workspace contains three crates. `kat-rs-datasource` owns protobuf decoding, mmap lifecycle, Arrow table creation, DataFusion registration, and JSON query output. `kat-rs-session` stores runtime state and delegates query calls. `kat-rs-cli` is the binary command surface.

**Tech Stack:** Rust 2024, clap, serde, prost, syn, memmap2, Arrow, DataFusion, serde_json, log, env_logger, tokio.

---

### Task 1: Workspace And Crate Skeleton

**Files:**
- Modify: `Cargo.toml`
- Delete: `src/main.rs`
- Create: `crates/kat-rs-datasource/Cargo.toml`
- Create: `crates/kat-rs-datasource/src/lib.rs`
- Create: `crates/kat-rs-session/Cargo.toml`
- Create: `crates/kat-rs-session/src/lib.rs`
- Create: `crates/kat-rs-cli/Cargo.toml`
- Create: `crates/kat-rs-cli/src/main.rs`

- [ ] Convert the root crate into a virtual workspace with members `kat-rs-datasource`, `kat-rs-session`, and `kat-rs-cli`.
- [ ] Add workspace dependencies for `anyhow`, `arrow-array`, `arrow-schema`, `clap`, `datafusion`, `env_logger`, `log`, `memmap2`, `prost`, `prost-build`, `serde`, `serde_json`, `syn`, `tempfile`, and `tokio`.
- [ ] Create minimal crate source files that compile.
- [ ] Run `cargo check --workspace`; expected result before implementation is compile success.

### Task 2: Datasource Hitrace Contract And Tests

**Files:**
- Create: `crates/kat-rs-datasource/proto/hitrace.proto`
- Create: `crates/kat-rs-datasource/build.rs`
- Create: `crates/kat-rs-datasource/tests/hitrace_query.rs`

- [ ] Add the minimal `HitraceTrace` / `HitraceEvent` protobuf contract.
- [ ] Generate prost structs from `hitrace.proto`.
- [ ] Write a failing test that serializes two hitrace events, builds a datasource from the file, and queries `select count(*) as count from hitrace_event`.
- [ ] Run `cargo test -p kat-rs-datasource --test hitrace_query`; expected failure before implementation is missing datasource API.

### Task 3: Datasource Implementation

**Files:**
- Create: `crates/kat-rs-datasource/src/config.rs`
- Create: `crates/kat-rs-datasource/src/hitrace.rs`
- Create: `crates/kat-rs-datasource/src/json.rs`
- Create: `crates/kat-rs-datasource/src/mmap.rs`
- Create: `crates/kat-rs-datasource/src/query.rs`
- Modify: `crates/kat-rs-datasource/src/lib.rs`

- [ ] Implement `DataSourceType`, `DataSourceConfig`, and `TraceDatasource::build`.
- [ ] Use `memmap2` while decoding, and drop mmap/file before returning from build.
- [ ] Parse the prost generated Rust struct AST in `build.rs` and generate Arrow builder code for table `hitrace_event`.
- [ ] Convert prost structs into Arrow arrays through the generated builder.
- [ ] Register the Arrow batch into DataFusion.
- [ ] Implement `query_json(sql)` returning `serde_json::Value`.
- [ ] Use `log` for diagnostics.
- [ ] Run `cargo test -p kat-rs-datasource --test hitrace_query`; expected result is pass.

### Task 4: Session Crate

**Files:**
- Create: `crates/kat-rs-session/tests/session_query.rs`
- Modify: `crates/kat-rs-session/src/lib.rs`

- [ ] Write a failing test that creates `Session`, builds a hitrace datasource, and queries JSON.
- [ ] Implement `Session::create`, `Session::build_datasource`, and `Session::query_json`.
- [ ] Return a clear error if query is called before datasource build.
- [ ] Run `cargo test -p kat-rs-session`; expected result is pass.

### Task 5: CLI Crate

**Files:**
- Create: `crates/kat-rs-cli/src/commands.rs`
- Create: `crates/kat-rs-cli/src/logging.rs`
- Modify: `crates/kat-rs-cli/src/main.rs`

- [ ] Implement `kat-rs query --source hitrace --file <path> --sql <sql>` with `clap`.
- [ ] Derive `serde` for CLI argument structures.
- [ ] Initialize `env_logger`.
- [ ] Write query JSON to stdout with `Write`, not `println!`.
- [ ] Write diagnostics to stderr or `log`.
- [ ] Add CLI unit tests for help, missing arguments, and unknown source.
- [ ] Run `cargo test -p kat-rs-cli`; expected result is pass.

### Task 6: End-To-End Verification And PR

**Files:**
- Modify as needed: `docs/superpowers/specs/2026-06-08-kat-rs-datasource-cli-session-design.md`

- [ ] Run `cargo fmt --all`.
- [ ] Run `cargo test --workspace`.
- [ ] Run `cargo clippy --workspace --all-targets -- -D warnings`.
- [ ] Run `cargo check --locked`.
- [ ] Run `cargo test --locked`.
- [ ] Run local PR Guard with a linked issue body.
- [ ] Commit the spec, plan, and implementation.
- [ ] Push branch `phybee/restart-architecture`.
- [ ] Create a PR against `maokelong/kat-rs:main` with validation evidence.
