---
status: accepted
---

# Run 发布要求可移植的 Output name

本决定部分取代 ADR-0008 与 ADR-0032 中 Output name 只需满足 ASCII snake_case
的决定。ADR-0010 与 ADR-0037 关于 Runtime 数据面所有权，以及 CLI 不派生或预检
候选 Output 文件的决定继续有效。

Run Output 的完整公开引用是 `(run_id, output_name)`，同一个 `output_name` 也直接
成为 `outputs/<output-name>.parquet` 的文件 stem。合法名称除完整匹配 ASCII
snake_case 外，还必须排除 Windows 设备保留名 `con`、`prn`、`aux`、`nul`、
`com1` 至 `com9` 和 `lpt1` 至 `lpt9`；KAT 不转换、转义或维护第二套物理名称。
该规则在 Pack Authoring API、Workflow Runtime 与 CLI 的不可信 Response acceptance
中一致执行。

Workflow Runtime 独占 Parquet 内容、Schema、完整 row count 与物化成功的证明。它只在
全部 Output 写入并关闭后返回 `run_workflow` 结果，直接 Runtime 随后写入完整 Response；
CLI 等待该进程退出，只接受退出码 `0` 加严格合法的 success Response，再发布
Manifest。CLI 不按 Output name 派生或预检候选文件，不解析 Parquet，也不核对 Schema
或 row count；Runtime failure、协议未完成、Response 非法或 Manifest 发布失败均不产生
Run。合法 success Response 是 Runtime 已完成其数据面责任的权威证明，而不是需要 CLI
通过文件系统观察重复证明的提示。

Output name 是公开业务身份，不属于候选私有值；candidate ID、candidate root 和完整
物理路径仍必须从 Runtime Diagnostic 中隔离。Manifest 和公开 Response 只记录逻辑
Output 引用与 metadata，不记录物理路径或声称文件系统事务。PACK 与 Runtime 不是
对抗性沙箱，本决定不试图防御受信任 PACK 主动伪造文件内容，或在 Workflow 返回后
遗留后台任务篡改候选。
