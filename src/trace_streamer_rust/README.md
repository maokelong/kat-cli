# Trace Streamer Rust

`trace_streamer_rust` is a Rust/DataFusion rewrite workspace for inspecting
OpenHarmony trace data.

## Purpose

The project is meant to make OpenHarmony trace analysis easier to run, test and
embed from Rust. It parses trace files into TraceStreamer-like tables, exposes
those tables through SQL, and provides both command-line and browser-based
inspection tools.

It is useful when you need to:

- inspect bytrace, htrace/profiler, rawtrace, hilog, hisysevent or perf data;
- query trace tables with SQL instead of writing custom ad hoc scripts;
- validate Rust parser behavior against C++ TraceStreamer SQLite exports;
- debug parser coverage while gradually rewriting C++ TraceStreamer logic in Rust.

## Workspace Crates

- `htrace-core`: shared request/result types, errors and query-engine trait.
- `htrace-model`: Arrow schemas, row models and `TraceTableBuilder`.
- `htrace-parser-harmony`: Harmony/OpenHarmony trace format detection and parsers.
- `htrace-query`: DataFusion registration, SQL execution and JSON result conversion.
- `htrace-engine-cli`: command-line inspect/query tools and C++ SQLite comparison helper.
- `htrace-web-ui`: local browser UI for inspecting tables and running SQL.

## Test Data

Checked-in fixtures live at `../test/resource` when commands are run from this
workspace directory:

- `ut_bytrace_input_thread.txt`: bytrace scheduler fixture used by parser tests.
- `ut_bytrace_input_full.txt`: bytrace sample suitable for CLI and Web UI demos.
- `rawtrace.bin`: rawtrace fixture.
- `perfCompressed.data`: perf fixture.

`pbreader.htrace` is intentionally not committed because the available sample is
larger than GitHub's regular file size limit.

## Run Tests

```powershell
cargo test --workspace
```

The fixture-backed tests read from `..\test\resource`, so they run without any
external repository checkout.

## CLI Usage

Inspect a trace and print table metadata:

```powershell
cargo run -p htrace-engine-cli --bin htrace-engine -- `
  inspect --trace ..\test\resource\ut_bytrace_input_full.txt --json
```

Run a SQL query:

```powershell
cargo run -p htrace-engine-cli --bin htrace-engine -- `
  query --trace ..\test\resource\ut_bytrace_input_full.txt `
  --sql "SELECT cpu, COUNT(*) AS slices FROM sched_slice GROUP BY cpu ORDER BY cpu" `
  --json
```

Other sample inputs can be inspected in the same way:

```powershell
cargo run -p htrace-engine-cli --bin htrace-engine -- `
  inspect --trace ..\test\resource\rawtrace.bin --json

cargo run -p htrace-engine-cli --bin htrace-engine -- `
  inspect --trace ..\test\resource\perfCompressed.data --json
```

## Web UI Usage

Start the local UI with a checked-in bytrace sample:

```powershell
cargo run -p htrace-web-ui -- `
  --trace ..\test\resource\ut_bytrace_input_full.txt `
  --listen 127.0.0.1:8787
```

Open `http://127.0.0.1:8787` in a browser. The UI shows parsed tables, row
counts and columns, and lets you run SQL such as:

```sql
SELECT cpu, COUNT(*) AS slices
FROM sched_slice
GROUP BY cpu
ORDER BY cpu;
```

## C++ Comparison

`compare-cpp-sqlite` compares selected Rust tables with C++ TraceStreamer SQLite
exports. It expects a trace input and a matching C++ SQLite database:

```powershell
cargo run -p htrace-engine-cli --bin compare-cpp-sqlite -- `
  --trace path\to\trace.htrace `
  --cpp-db path\to\cpp_trace.db `
  --html-output target\compare_validation_report.html
```
