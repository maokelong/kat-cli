# CLI Run Review Protocol

本协议指导 LLM 通过 Cargo 执行 `kat-rs` binary 的 `analyze` 子命令，并审阅 CLI 产出的 run artifacts。

## 输入

- db path。
- pack root。
- analysis id。
- target process。
- marker。
- run id。
- run root。
- selected strategy。
- pack mapping 文档，若存在。

## CLI 命令

只能通过 Cargo 启动 `kat-rs` binary 执行（`kat-rs analyze` 语义）：

```sh
cargo run -p kat-rs-cli -- analyze \
  --db DB_PATH \
  --pack PACK_ROOT \
  --analysis ANALYSIS_ID \
  --target-process TARGET_PROCESS \
  --marker MARKER \
  --run-id RUN_ID \
  --run-root RUN_ROOT
```

禁止直接查询 SQLite、直接运行 SQL、直接写 run artifacts。

## Artifact 检查

命令成功后必须检查：

- `RUN_ROOT/RUN_ID/plan.json`
- `RUN_ROOT/RUN_ID/state.json`
- `RUN_ROOT/RUN_ID/evidence.jsonl`
- `RUN_ROOT/RUN_ID/report.md`
- `RUN_ROOT/RUN_ID/checklist.md`

`checklist.md` 是 CLI 生成的可读 artifact；它不是 skill repo 中维护的模板，也不是机器执行 source of truth。

缺任一文件都视为 run review 失败。

## Plan Review

检查 `plan.json`：

- 是否是请求的 analysis id。
- 是否包含 strategy 所需的 seed step。
- 是否包含 strategy 所需的 graph walk 或其他 analysis step。
- 是否包含 report render step。

不要修改 `plan.json`。

## State Review

检查 `state.json`：

- `root` 是否包含目标对象和窗口。
- `frontier` 是否反映 graph walk 当前节点。
- `graph.visited` 或 `decisions` 是否记录已选边或未选原因。

不要把 state 缺口补写进 `state.json`。

## Evidence Review

检查 `evidence.jsonl`：

- 每行必须是 JSON object。
- `status: ok` 可以作为 facts 引用。
- `status: partial` 可以引用，但必须进入 uncertainty。
- 空 evidence 或缺少关键字段时，只能报告 gap 或 uncertainty。

## Report Review

检查 `report.md`：

- 必须分为 `# Facts`、`# Inferences`、`# Uncertainty`。
- facts 必须能从 evidence 或 state 中追溯。
- inferences 必须说明依赖哪些 facts。
- uncertainty 必须包含缺失表、缺失 marker、缺失 waker、深度截断或 pack/runtime gap。

## 输出

最终回复包含：

- CLI command 摘要。
- artifacts 检查结果。
- strategy coverage。
- facts。
- inferences。
- uncertainty。
- gaps。
