# DataFusion Signature Runner 落地方案

目标: 把一次已经执行并确认的冷启动分析流程固化成可批量重放的 DataFusion 查询脚本。

范围收敛:

- 不做数据规范层。
- 不抽象多种查询引擎。
- 默认 trace 已经能进入 DataFusion 表。
- 脚本只负责发送 SQL、保存证据 JSON、执行 signature 判定。

当前最小脚本:

```text
cases/wechat-cold-start/tools/datafusion_signature_runner.py
```

它通过 kat-rs Web UI 的 DataFusion 查询接口执行固定 SQL:

- `harmony_process_candidates`
- `harmony_cold_start_tag_by_process`
- `harmony_cold_start_anchor_select`
- `harmony_cold_start_phase_breakdown`
- `harmony_process_critical_path_in_range`
- `harmony_main_thread_states_by_phase`
- `harmony_cpu_cluster_mapping`
- `harmony_critical_path_cpu_cluster_time`

## 单 trace 执行

使用当前 Web UI active dataset:

```powershell
python cases\wechat-cold-start\tools\datafusion_signature_runner.py `
  --server http://127.0.0.1:8787 `
  --out-dir cases\wechat-cold-start\signature-output\test-htrace
```

上传并分析一个 trace:

```powershell
python cases\wechat-cold-start\tools\datafusion_signature_runner.py `
  --server http://127.0.0.1:8787 `
  --trace tests\test.htrace `
  --out-dir cases\wechat-cold-start\signature-output\test-htrace
```

输出:

```text
cases/wechat-cold-start/signature-output/<case>/
  signature_result.json
  signature_result.md
```

## 批量执行

PowerShell 示例:

```powershell
$traces = Get-ChildItem D:\traces -Filter *.htrace
foreach ($trace in $traces) {
  $name = [IO.Path]::GetFileNameWithoutExtension($trace.Name)
  python cases\wechat-cold-start\tools\datafusion_signature_runner.py `
    --server http://127.0.0.1:8787 `
    --trace $trace.FullName `
    --out-dir "cases\wechat-cold-start\signature-output\$name"
}
```

后续可把每个 `signature_result.json` 汇总成 CSV。

## 当前 Signature

脚本内置 signature id:

```text
harmony_wechat_cold_start_js_load
```

判定条件:

| predicate | 条件 |
| --- | --- |
| `max_phase_is_launch_ability` | 最大阶段是 `C_launch_ability_to_transaction` |
| `max_phase_ratio_high` | 最大阶段占总窗口 >= 40% |
| `main_thread_running_dominant` | 最大阶段主线程 running ratio >= 70% |
| `js_load_hotspot` | 关键路径 callstack 命中 JS/module load 关键词 |
| `not_small_core_issue` | small core ratio < 5% |

全部满足则 `status=match`。

## LLM 参与边界

批量运行时不需要 LLM 参与判定。

允许 LLM 做:

- 根据 `signature_result.json` 写自然语言说明。
- 对 `inconclusive` 做原因解释。
- 帮助新增 signature 的阈值说明。

不允许 LLM 做:

- 临场选 anchor。
- 手算耗时或占比。
- 改阈值后再判断 match。
- 把候选关键路径直接写成唯一根因。
