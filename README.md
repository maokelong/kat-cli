# kat-rs

`kat-rs` hosts a Rust rewrite workspace for OpenHarmony TraceStreamer under
`src/trace_streamer_rust`.

## What This Project Solves

This project provides a Rust-native path for OpenHarmony trace inspection:

- parse OpenHarmony trace inputs such as bytrace text, htrace/profiler data, rawtrace, hilog, hisysevent and perf data;
- normalize parsed records into TraceStreamer-like Arrow tables;
- run SQL over the parsed tables with DataFusion;
- inspect traces from a command-line tool or a local browser UI.

## Layout

- `src/trace_streamer_rust`: Rust workspace for parser, model, query engine, CLI and Web UI.
- `src/trace_streamer_rust/test/resource`: small checked-in trace samples used by parser/query tests and local demos.

Large profiler captures such as `pbreader.htrace` are not committed because they
exceed GitHub's regular file size limit. Use local captures with the same CLI and
Web UI commands when larger validation data is needed.

## Quick Start

```powershell
cd src\trace_streamer_rust
cargo test --workspace
```

Inspect a checked-in bytrace sample:

```powershell
cargo run -p htrace-engine-cli --bin htrace-engine -- `
  inspect --trace test\resource\ut_bytrace_input_full.txt --json
```

Run SQL over the same sample:

```powershell
cargo run -p htrace-engine-cli --bin htrace-engine -- `
  query --trace test\resource\ut_bytrace_input_full.txt `
  --sql "SELECT cpu, COUNT(*) AS slices FROM sched_slice GROUP BY cpu ORDER BY cpu" `
  --json
```

Start the local Web UI:

```powershell
cargo run -p htrace-web-ui -- `
  --trace test\resource\ut_bytrace_input_full.txt `
  --listen 127.0.0.1:8787
```

Then open `http://127.0.0.1:8787` and query the parsed tables from the browser.
