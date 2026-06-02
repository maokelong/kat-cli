# Rust Rewrite Status

更新时间: 2026-06-01

## 当前验证口径

- 新 workspace: `kat-rs-pr` 根 workspace。
- 已提交小样例: `tests/fixtures/traces`。
- 本地大样例: `tools/trace-validation/local_resource`，由原 C++ TraceStreamer 项目的样例目录复制而来，已在 `.gitignore` 中排除。
- 本地验证脚本: `tools/trace-validation/scripts/local_validate_all.ps1`。
- 最新报告:
  - `target/local_fixture_inspect_report.json`
  - `target/compare_validation_report.json`
  - `target/compare_validation_report.html`

## 本地全量样例解析

执行命令:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\trace-validation\scripts\local_validate_all.ps1 -CopyLocalResources
```

结果:

- `cargo test --workspace`: passed。
- 共扫描 24 个 trace/resource 文件。
- Rust inspect 成功 22 个，失败 2 个。
- 失败样例:
  - `htrace.zip`: 当前 Rust parser 未处理 zip 包装，按 htrace length-prefixed segment 解析时报 segment length 越界。
  - `zlib.htrace`: 当前 Rust parser 未处理 zlib 压缩包装，按 htrace length-prefixed segment 解析时报 segment length 越界。
- 已提交的 4 个小样例全部解析成功:
  - `perfCompressed.data`
  - `rawtrace.bin`
  - `ut_bytrace_input_full.txt`
  - `ut_bytrace_input_thread.txt`

## C++ 对比范围

当前机器没有可直接运行的正式 C++ `trace_streamer.exe`，WSL 也未安装；因此本轮使用已经存在的 C++ SQLite 导出进行对比:

- `cpp_htrace_pbreader.db`
- `cpp_bytrace_full.db`
- `cpp_perf_compressed.db`

`rawtrace.bin` 在 Rust 侧解析成功，但没有找到对应的 `cpp_rawtrace.db`，所以暂未纳入 C++ 行数对比。

## 最新 Gap

### htrace_pbreader

20 张目标表:

- 已对齐 18 张。
- 未对齐:
  - `data_dict`: C++ 150785 / Rust 121088，delta -29697。
  - `args`: C++ 748323 / Rust 112635，delta -635688。

全表行数中的主要差异:

- `measure_filter`: C++ 2485 / Rust 20，delta -2465。
- `perf_callchain`: C++ 1361093 / Rust 0。
- `perf_files`: C++ 5131 / Rust 0。
- `perf_report`: C++ 4 / Rust 0。
- `perf_sample`: C++ 134954 / Rust 0。
- `perf_thread`: C++ 279 / Rust 0。
- `process`: C++ 257 / Rust 462，delta +205。
- `thread`: C++ 463 / Rust 288，delta -175。

C++ 有非零数据但 Rust 尚未建模的表:

- `bio_latency_sample`: 46
- `clk_event_filter`: 109
- `clock_snapshot`: 6
- `data_type`: 4
- `datasource_clockid`: 7
- `device_info`: 1
- `device_state`: 1
- `ebpf_callstack`: 9568
- `file_system_sample`: 22155
- `hisys_all_event`: 80
- `paged_memory_sample`: 3107
- `stat`: 485
- `trace_config`: 2
- `trace_range`: 1

### bytrace_full

20 张目标表:

- 已对齐 19 张。
- 未对齐:
  - `data_dict`: C++ 89 / Rust 44，delta -45。

全表行数中的主要差异:

- `cpu_measure_filter`: C++ 1 / Rust 0。
- `instant`: C++ 16 / Rust 0。
- `irq`: C++ 8 / Rust 3。
- `measure`: C++ 1 / Rust 0。
- `measure_filter`: C++ 1 / Rust 0。
- `process`: C++ 6 / Rust 7，delta +1。
- `raw`: C++ 16 / Rust 0。
- `thread`: C++ 8 / Rust 7，delta -1。

C++ 有非零数据但 Rust 尚未建模的通用元信息表:

- `data_type`: 4
- `device_info`: 1
- `stat`: 485
- `trace_range`: 1

### perf_compressed

20 张目标表:

- 已对齐 19 张。
- 未对齐:
  - `data_dict`: C++ 543 / Rust 3，delta -540。

全表行数中的主要差异:

- `process`: C++ 1 / Rust 0。
- `thread`: C++ 1 / Rust 0。

C++ 有非零数据但 Rust 尚未建模的通用元信息表:

- `data_type`: 4
- `device_info`: 1
- `stat`: 485
- `trace_range`: 1

## 后续优先级

- shared dictionary / args:
  - `htrace_pbreader.args` 仍是最大 gap，主要缺 C++ raw ftrace 字段、内部 argset、统计类 argset 的批量写入。
  - `data_dict` 仍缺 C++ 默认字典、raw ftrace 字段名/字段值、stat/meta 相关字符串。
- perf-in-htrace:
  - `pbreader.htrace` 中 C++ 已解析 perf 表族，Rust 当前只在独立 `perfCompressed.data` 上解析 perf，尚未接入 htrace/profiler 内嵌 perf 数据流。
- bytrace:
  - `instant`、`raw`、`measure`、`cpu_measure_filter` 与 C++ 行数尚未对齐。
  - `irq` 行数仍少 5 行，需要继续对齐 hardirq/softirq 文本事件映射。
- 压缩/包装格式:
  - `htrace.zip` 和 `zlib.htrace` 当前失败，需要补 zip/zlib 解包后再进入 htrace parser。
- C++ 对比覆盖:
  - 需要拿到正式 C++ `trace_streamer.exe` 或新增对应 SQLite 导出，才能把 `rawtrace.bin`、`trace_small_10.systrace`、更多 htrace 小样例纳入 C++ 行数对比。
