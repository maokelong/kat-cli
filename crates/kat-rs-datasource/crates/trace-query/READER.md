# trace-query

`trace-query` is the datasource-owned SQL layer. It registers parsed trace data
with DataFusion and exposes reusable query/session primitives.

## Responsibilities

- Register `trace_model::ParsedTrace` Arrow batches as DataFusion tables.
- Execute SQL and return columns, rows, status and query statistics.
- Convert Arrow query output into JSON-friendly `QueryResult` values with empty
  result handling, truncation metadata and common primitive type support.
- Provide `HtraceDataFusionEngine` and `ParsedTraceQuerySession` for
  open/inspect/query/close and parse-once-query-many workflows.
- Own query-facing request/result types, errors, handles and the
  `TraceQueryEngine` trait formerly stored in the standalone core crate.

## Boundaries

- This crate does not parse raw trace files and does not mutate model data.
- Format-specific parsing belongs to `trace-parser`.
- Table schemas and row builders belong to `trace-model`.
- CLI, Web UI and future service adapters should call the datasource facade
  first; `trace-query` remains an implementation dependency of datasource.
