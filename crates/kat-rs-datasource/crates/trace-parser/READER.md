# trace-parser

`trace-parser` is the datasource-owned parser layer for Harmony/OpenHarmony
trace inputs.

## Responsibilities

- Detect bytrace, htrace/profiler, rawtrace, hilog, hisysevent and perf inputs.
- Parse inputs into `trace_model::ParsedTrace` while preserving table and field
  compatibility with TraceStreamer-style SQL.
- Provide `HarmonyTraceParser`, format detection, `parse_trace_file` and
  `parse_trace_bytes` entry points.
- Route htrace/profiler plugin payloads to ftrace, CPU, disk I/O, memory,
  process and ArkTS parser logic.
- Parse bytrace scheduler, wakeup, trace marker, binder and softirq events into
  scheduler, IRQ, raw event and shared callstack tables.

## Boundaries

- Parser code owns format detection, protocol decoding, timestamp handling,
  state machines and plugin routing.
- Arrow schemas and final batch construction belong to `trace-model`.
- SQL execution and UI presentation belong to datasource/query callers, not to
  this crate.
- Parser-facing errors live in `trace-parser`; query-facing errors live in
  `trace-query`.
