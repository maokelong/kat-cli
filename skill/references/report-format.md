# 最终报告格式

最终报告必须使用中文撰写，面向用户可直接阅读和复核。报告必须引用 atomic 输出字段、artifact 路径或 validation 结果；不得使用没有证据支撑的判断。

所有面向用户或评审阅读的 Markdown supporting artifacts 也必须使用中文标题和中文段落，包括 `summary.md`、`topdown-brief.md`、`strategy-selection.md`、`deep-analysis-summary.md` 与人工 handoff。机器可读日志、JSON、CSV、YAML 字段名可保留英文。

## 硬性要求

- 顶部必须有 `## 中文摘要`，用 3-5 条要点概括结论、主证据、目标进程、关键负证据和下一步。
- 目标进程选择必须可复核：写明候选来源、使用的字段、匹配规则和最终选择原因。除非有明确字段支持，不得使用 "clearest"、"obvious"、"明显" 等不可复核表述。
- CPU contention 段必须同时写正证据和负证据，并区分“窗口运行负载证据/竞争者证据”和“目标进程受 CPU contention 影响的因果证据”；不得把窗口内存在高负载或竞争者直接写成目标进程根因。若目标进程未出现在 returned rows、top returned rows 或 top runnable competitors 中，必须明确写出这一点，并说明它如何限制结论强度。
- 引用 `sched_latency_overview` 时，除非字段能证明返回线程就是主线程，否则只能表述为“`sched_latency_overview` 返回线程的 runnable_wait_ms”或“目标进程相关线程的 runnable_wait_ms”，不得直接写成“目标主线程 runnable_wait_ms”。
- 必须包含 `## Validation notes`，列出 pre-advance validation findings、最终 validation 结果和 `ok=true` 是否成立；同时列出本 run 中实际存在的 `final_report validate`、`completed validate`、`final_report advance` 三类 command-output 记录的 stdout artifact 与 stderr artifact 具体路径，或等价命令记录的 stdout/stderr artifact 具体路径。每个 command-output artifact 条目必须同步写明对应 command id/序号或简短命令文本，例如 `034 validate-final_report`、`035 advance-final_report-to-completed`、`036 validate-completed`；若 stderr 为空，也必须明确写 `stderr artifact exists and is empty` 或中文等价说明（如 `stderr artifact 存在且为空`）。若 `ok=true` 不成立，报告不得给出完成性结论。
- 中文最终报告中优先使用中文空 stderr 表述（如 `stderr artifact 存在且为空`）；只有机器字段或外部原始输出可保留英文。
- 若 replay、atomic、profile route 或任何受控 htrace 命令失败，最终报告顶部必须明确标记为未完成或失败，不得只在末尾“残留风险”中提到。
- 必须汇总性能可观测性证据，引用 E2E/命令记录 artifact，并覆盖命令 `started_at`、`completed_at`、`elapsed_ms`、`exit_code`、`stdout_bytes`、`stderr_bytes`、atomic/replay 耗时、trace 大小、`htrace`/`trace_processor` 版本、执行性能限制，以及 trace processor spawn/load/query/RSS/cache 观测字段。字段要求见 `references/e2e-observability.md`。
- 优化建议必须转成具体下一步分支：每条建议都要写触发条件、要运行的 atomic 或 replay、预期判定字段和可能分支，不写泛泛的“继续观察/进一步优化”。

## 推荐结构

```markdown
## 中文摘要

- 结论：
- 目标进程：
- 主证据：
- 负证据：
- 下一步分支：

## 结论

说明当前问题是否成立、影响范围和置信度。每个结论后标注证据来源。

## 目标进程选择

- 候选来源：
- 使用字段：
- 匹配规则：
- 最终目标：
- 不确定性：

## 证据链

按阶段列出关键 atomic、每个证据小节对应的 artifact 路径、字段和值。区分事实、推断和不确定性；不要只在摘要中给目录路径后让读者自行猜测小节对应文件。

## CPU contention

- 窗口运行负载证据/竞争者证据：
- 目标进程受 CPU contention 影响的因果证据：
- 负证据：
- 结论限制：

## 分支路径

说明为什么进入当前分析分支，哪些分支被排除，排除依据是什么。

## 具体下一步

每条下一步写成可执行分支：

- 触发条件：
- 执行命令或 atomic：
- 预期字段：
- 分支 A：
- 分支 B：

## Validation notes

- Pre-advance validation findings：
- Final validation：
- ok=true：
- Command-output artifacts：
  - `034 validate-final_report` final_report validate：
    - stdout artifact：
    - stderr artifact：（若为空，写明 `stderr artifact exists and is empty` 或 `stderr artifact 存在且为空`）
  - `035 advance-final_report-to-completed` final_report advance：
    - stdout artifact：
    - stderr artifact：（若为空，写明 `stderr artifact exists and is empty` 或 `stderr artifact 存在且为空`）
  - `036 validate-completed` completed validate：
    - stdout artifact：
    - stderr artifact：（若为空，写明 `stderr artifact exists and is empty` 或 `stderr artifact 存在且为空`）
- 性能可观测性证据：
  - 命令记录 artifact：
  - 命令级耗时/退出码/输出字节数：
  - atomic/replay 耗时：
  - trace 大小：
  - htrace/trace_processor 版本：
  - 性能限制：
  - 资源/缓存观测：逐项列出 `resource_observability` 的 key/value，包括 `trace_processor_spawn_count`、`trace_load_ms`、`query_ms`、`peak_rss_bytes`、`cache_hits`、`cache_misses` 和 `unavailable_reason`。
- 残留风险：

## Replay plan

给出 replay plan 或 signature artifact 路径；若未生成，说明原因和限制。
```

## 表述约束

- 不把 trace processor stderr、加载日志或临时 shell 输出当作用户可消费证据。
- 不把历史 `validation/` 旧产物当作当前 run evidence。
- 不用“疑似”“可能很高”“目标主线程”等模糊或未证实归属的词单独支撑结论；必须接证据字段或明确写入不确定性。
- 当字段缺失或 atomic returned rows 为空时，必须把缺失本身写入负证据或限制条件。
- 当 atomic 输出是 header-only CSV/raw_stdout，或只有表头没有数据行时，报告写成“仅表头、无数据行”，不要写成可能被误读为命令未返回的“无返回行”。
