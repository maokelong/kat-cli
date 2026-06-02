# kat-rs Compatibility Validation

Use the datasource validation command to compare checked-in golden suites:

```powershell
cargo run -p kat-rs-cli -- datasource validate `
  --trace tests\fixtures\traces\ut_bytrace_input_full.txt `
  --query-suite tests\golden\bytrace_full `
  --json
```

Large compatibility samples should stay outside git. Store local comparison
artifacts from C++ TraceStreamer or Perfetto beside the sample set, then add a
small checked-in golden suite that captures the stable SQL rows needed for CI.

Suggested local layout:

```text
compat-samples/
  sample-name/
    input.trace
    trace_streamer_cpp/
      query-name.expected.json
    perfetto/
      query-name.expected.json
    kat-rs/
      query-name.actual.json
```

Keep repository fixtures small. Add larger samples only through an external
artifact store or local benchmark pack.
