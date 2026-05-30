# E2E 性能可观测性

E2E、replay 和最终报告必须记录命令级与 atomic 级耗时，保证性能评审可以复核分析过程本身的开销。

## 必填字段

每条 E2E 命令记录至少包含：

- `command`：脱敏后的命令或 atomic/replay id。
- `started_at`：命令开始时间，使用 ISO 8601 带时区格式。
- `completed_at`：命令结束时间，使用 ISO 8601 带时区格式。
- `elapsed_ms`：墙钟耗时，单位毫秒。
- `exit_code`：进程退出码；若命令未启动，写 `not_started` 并说明原因。
- `stdout_bytes` / `stderr_bytes`：标准输出和标准错误字节数。
- `artifact`：命令产生或更新的 evidence、artifact、report 路径；无产物时写 `none`。

每份 run 还必须汇总：

- `atomic_timings`：每个 `htrace atomic run` 的 atomic id、trace、elapsed_ms、exit_code 和 artifact 路径。
- `replay_timings`：每个 `htrace replay run` 或 `htrace replay batch` 的 replay 文件、trace 列表、elapsed_ms、exit_code 和 artifact 路径。
- `trace_size_bytes`：输入 trace 文件大小；批量 replay 时逐个 trace 记录。
- `htrace_version`：`htrace version` 输出。
- `trace_processor_version`：通过当前 `HTRACE_TRACE_PROCESSOR` 后端可复核的版本或文件版本；无法取得时写明检查命令和失败原因。
- `performance_limits`：本次执行使用的限制，例如 `--jobs`、超时、机器内存约束、是否避免并行加载过多 trace。
- `resource_observability`：记录 trace processor 相关资源观测字段，至少包含 `trace_processor_spawn_count`、`trace_load_ms`、`query_ms`、`peak_rss_bytes`、`cache_hits`、`cache_misses`。当前工具链无法精确取得时，必须写 `not_available` 或 `estimate`，并说明原因，不能省略字段。

## 记录位置

- E2E 验证产物应优先把原始命令记录保存为结构化 JSON 或 Markdown 表格。
- 最终报告的 `Validation notes` 必须汇总上述证据，并引用对应 artifact 路径。
- 若缺失任一必填字段，报告必须把缺失项列为残留风险，不能给出“性能开销已可复核”的结论。若字段以 `not_available` 记录，报告必须说明这是工具链限制还是本次未采集。

## 最小表格模板

```markdown
| command | started_at | completed_at | elapsed_ms | exit_code | stdout_bytes | stderr_bytes | artifact |
| --- | --- | --- | ---: | --- | ---: | ---: | --- |
| htrace atomic run ... | 2026-05-29T10:00:00+08:00 | 2026-05-29T10:00:02+08:00 | 2000 | 0 | 1234 | 120 | evidence/overview/trace_sanity_check.json |
```
