# Trace Streamer Rust MVP

This workspace is the Rust/DataFusion rewrite prototype for OpenHarmony `.htrace`
and bytrace text analysis.

## Crates

- `htrace-core`: shared request/result types and engine trait.
- `htrace-model`: canonical `htrace.v1` Arrow schemas and builders.
- `htrace-parser-harmony`: `.htrace` / bytrace text reader and scheduler parser MVP.
- `htrace-query`: DataFusion table registration and SQL execution.
- `htrace-engine-cli`: `inspect` and `query` CLI.

## Current Scope

The parser currently supports:

- profiler file header framing: 1024-byte header + length-prefixed protobuf segments;
- `ProfilerPluginData`;
- `ftrace-plugin` -> `TracePluginResult`;
- `FtraceCpuDetailMsg` -> `FtraceEvent`;
- bytrace text rows matching `TASK-PID (TGID) [CPU] FLAGS TIMESTAMP: EVENT: ...`;
- `sched_switch` -> `sched_slice`, `thread_state`, `thread`, and `process`;
- unsupported plugin/event fallback into `raw_event`.

All timestamps and durations in canonical tables are signed nanoseconds.

## Example

```powershell
cargo run -p htrace-engine-cli --bin htrace-engine -- `
  inspect --trace ..\test\resource\pbreader.htrace --json

cargo run -p htrace-engine-cli --bin htrace-engine -- `
  query --trace ..\test\resource\pbreader.htrace `
  --sql "SELECT cpu, COUNT(*) AS slices FROM sched_slice GROUP BY cpu ORDER BY cpu" `
  --json
```

## Verification

```powershell
cargo test
```
