# kat-rs Pack Run MVP Verification

## Commands

```powershell
cargo test
python -m pytest python/tests/test_sdk_runtime_contract.py -q
cargo build -p kat-rs-cli

$env:KAT_RS_PYTHON = "python"
$db = if ($env:KAT_RS_E2E_DB) { $env:KAT_RS_E2E_DB } else { "D:\work\kat_rs\0709\kat-rs\test\test.db" }
$dataset = Join-Path $env:TEMP "kat-rs-sqlite-dataset"
$run = Join-Path $env:TEMP "kat-rs-pack-run"
Remove-Item -Recurse -Force $dataset,$run -ErrorAction SilentlyContinue
.\target\debug\kat-rs.exe dataset materialize sqlite $db $dataset
.\target\debug\kat-rs.exe dataset query $dataset --sql "select count(*) as count from thread_state"
.\target\debug\kat-rs.exe pack inspect packs\openharmony-critical-path --json
.\target\debug\kat-rs.exe pack run packs\openharmony-critical-path wechat_first_frame_critical_path --dataset $dataset --run-dir $run
Get-Content (Join-Path $run "manifest.json")
```

## Result

- Rust tests: passed. Two local E2E tests in `pack_run_contract` were ignored as expected.
- Python contract tests: `7 passed`.
- CLI build: `cargo build -p kat-rs-cli` passed.
- SQLite materialization: `test/test.db` was materialized into a temporary Parquet catalog.
- Dataset query: `thread_state` count was `410551`.
- Pack inspect: discovered `wechat_first_frame_critical_path` workflow and `critical_path` compute.
- Pack run: `kat-rs pack run` completed with manifest `status: success`.
- Artifacts: `artifacts/path_nodes.parquet` and `artifacts/path_edges.parquet` existed and were non-empty.
- Artifact facts: `path_nodes` had `146` logical node rows and `path_edges` had `145` rows. Duplicate logical nodes by `(depth, itid, window_start, window_end)` were `0`. `path_nodes` included `.tencent.wechat` / `itid=405`, `tid=15040` first-frame window critical-path nodes.
