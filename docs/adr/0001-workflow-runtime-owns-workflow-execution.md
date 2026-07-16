---
status: accepted
---

# Workflow Runtime 独占 Workflow 执行面

KAT 将 Dataset 之后的 SQL、DataFrame、PACK Workflow 和 Run Output 写出统一放在 Workflow Runtime 内执行，DataFusion 只属于这个 Query Engine 边界；`kat run`、`kat query` 与 Output Query 都启动同一种短命 Runtime。KAT CLI 只保留平台入口、操作编排、进程、候选执行与 Run 发布职责，Datasource 通过 Arrow 与 Parquet 完成领域解码和 Dataset Storage；两者都不链接 DataFusion。`kat query --sql` 的 CLI 只接收并转交一条文本，不解析或执行 SQL；唯一 Runtime 执行面避免了版本、注册、Dataset 接受标准和错误语义漂移，也避免为跨进程查询重建第二个 DataFusion 结果层。CLI 与 Runtime 只通过 JSON 文件交换控制信息，通过 Parquet 文件交换表数据，不跨进程传递 DataFrame、Logical Plan 或内存 Arrow buffer；验证关注一条 Data Import 到 Runtime 查询的跨边界集成链路，而不是要求两侧内部依赖版本相同。

生产 `kat run` 启动 Workflow Runtime 前，Rust CLI 预分配私有候选 UUID 并创建 KAT Data Home 的 `runs/<candidate-id>/`；不维护独立 Run registry。CLI 只把该 UUID 与候选目录写进 `run_workflow` request，Runtime 只能写入该目录；Operation log 由 CLI 独占，不进入 request。Runtime 将执行结果写入 CLI 分配的随机临时 Runtime Response 文件，它是以 `status`、`result` 与 `error` 表达 success/failure 的短命 IPC 结果，不是可查询状态。CLI 排空标准流、回收进程、确认 Runtime 以 `0` 退出并严格验证 Runtime Response 的 success `result`，并完整写入及 flush Operation log 后，才把 CLI 持有的候选 UUID、PACK、Workflow 与可选 canonical Dataset path，同 Runtime `result` 中新产生的 effective inputs 和 Outputs 合成为不复制 Runtime Response 的 status/result wrapper 的 Run Manifest，写入 CLI 自己创建的同目录临时文件，并通过成熟的临时文件持久化能力发布为唯一的 `manifest.json`。Runtime Response 不会被直接重命名为 Run Manifest。此时候选 UUID 才成为公开 Run ID，目录才成为 Run，成功 KAT Response 才返回 `run_id`。任一步失败都不产生 Run 或 `manifest.json`，failure `kat run` Response 不含 `result`；候选目录、日志、临时文件清理失败留下的随机残留和部分 Output 可以保留为诊断证据，但 `kat query` 一律忽略。PACK test 的临时执行由 harness 在 pytest `tmp_path` 下创建，复用执行和 Output 布局但不进入 Data Home，也不成为 `kat query` 目标。
