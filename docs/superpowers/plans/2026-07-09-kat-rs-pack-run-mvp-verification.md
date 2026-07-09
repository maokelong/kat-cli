# kat-rs Pack Run MVP 验证记录

## 命令

```powershell
cargo test
python -m pytest python/tests/test_sdk_runtime_contract.py -q
cargo build -p kat-rs-cli

$env:KAT_RS_PYTHON = "python"
$db = "D:\work\kat_rs\0709\kat-rs\test\test.db"
$dataset = Join-Path $env:TEMP "kat-rs-sqlite-dataset"
$run = Join-Path $env:TEMP "kat-rs-pack-run"
Remove-Item -Recurse -Force $dataset,$run -ErrorAction SilentlyContinue
.\target\debug\kat-rs.exe dataset materialize sqlite $db $dataset
.\target\debug\kat-rs.exe dataset query $dataset --sql "select count(*) as count from thread_state"
.\target\debug\kat-rs.exe pack inspect packs\openharmony-critical-path --json
.\target\debug\kat-rs.exe pack run packs\openharmony-critical-path wechat_first_frame_critical_path --dataset $dataset --run-dir $run
Get-Content (Join-Path $run "manifest.json")
```

## 结果

- Rust 测试：通过；`pack_run_contract` 中 2 个本地依赖 E2E 测试按预期 ignored。
- Python 合同测试：`7 passed`。
- CLI 构建：`cargo build -p kat-rs-cli` 通过。
- SQLite 物化：`test/test.db` 成功物化为临时 Parquet catalog。
- 数据集查询：`thread_state` 计数为 `410551`。
- Pack inspect：发现 `wechat_first_frame_critical_path` workflow 和 `critical_path` compute。
- Pack run：通过 `kat-rs pack run` 执行成功，manifest `status` 为 `success`。
- 产物：`artifacts/path_nodes.parquet` 和 `artifacts/path_edges.parquet` 存在且非空。
- 产物事实：`path_nodes` 共 `1802` 行，`path_edges` 共 `145` 行；`path_nodes` 包含 `.tencent.wechat` / `itid=405`、`tid=15040` 的首帧窗口关键路径节点。
