# trace-model

`trace-model` is the datasource-owned data model layer. It turns parser output
into Arrow `RecordBatch` values that can be registered by the SQL layer.

## Responsibilities

- Define TraceStreamer-compatible table schemas, table names, field names and
  primitive data types.
- Provide row structs and `TraceTableBuilder` write APIs so parsers can push
  domain rows without touching Arrow internals.
- Build `ParsedTrace` during `finish`, including trace id, time bounds, clock
  domain and all table batches.
- Maintain lightweight indexes and backfill helpers such as string interning,
  argset allocation, callstack updates, measure duration updates and JS heap
  self-size traversal.

## Boundaries

- This crate does not detect input formats or decode binary protocols.
- This crate does not execute SQL or format CLI/Web UI responses.
- Schema changes affect parser, query, CLI and Web UI behavior, so they should
  be covered by datasource schema and golden tests.
