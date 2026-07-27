---
status: accepted
---

# Output Query 信任本地 Runtime

本决定取代 ADR-0008、ADR-0010 与 ADR-0019 中仅针对 `query_run` 的 Output ID、
固定查询限制、严格私有 DTO 和数据来源门禁条款。Run publication 已按 ADR-0055
使用 `(run_id, output_name)` 作为完整公开引用，且不维护第二套物理名称；因此 CLI
从最终 Manifest 取得全部 Output names，Runtime 按 canonical Run path 与名称确定性
打开 `outputs/<output-name>.parquet`。这项取代不改变上述 ADR 的其他决定，也不改变
ADR-0055 归属的 Run publication。

KAT CLI、Workflow Runtime 与 Bundled Python Host 是同版本构建并原子发布的可信单元。
Skill、模型和用户不构造私有 Runtime IPC；自行混装或篡改发布载荷不属于受支持边界。
`query_run` request 只携带 Runtime 执行所需的 canonical Run path、全部 Output names、
未经 CLI 解释的 SQL，以及当前可用时才出现的 Resolved Dataset。Python 只把同版本
request 构造成内部 typed value，不重复验证 CLI 已经确定的 UUID、canonical path、
Output name 或字段全集。Rust 只确认 Host 退出、Response 文件存在且完整 JSON 可以
解码；内部实现一致性由 typed construction 与真实 CLI 到 Bundled Host 的契约测试承担，
不在两种语言中维护平行的 Arrow 类型和逐 cell 防御矩阵。

Workflow Runtime 是 Arrow 到公开 Query Result JSON 的唯一投影 owner。它生成 ordered
columns 与 positional rows，并在同一处完成 64 位整数、decimal、有限浮点、string、
UTC nanosecond timestamp 与 null 的无损 JSON 投影；不支持的 Arrow type 整体失败。
Runtime 私有 result 不回显 Dataset 状态，CLI 只把该结果与自己持有的 Dataset 当前状态
组装成公开 KAT Response，不重新解析 Arrow type string 或逐 cell 复核 Runtime 结果。公开
`dataset` 固定为三个互斥分支：未提供时是 `{"status":"not_provided"}`；当前可用时是
`{"status":"available","path":"..."}`；记录路径当前不可用时是
`{"status":"unavailable","path":"...","cause":"..."}`。后两者表示查询当下状态。

用户 SQL 是受信任的本地输入。DataFusion `SQLOptions` 继续拒绝 DDL、DML 与 session
statement，只表达 `query` 不修改 KAT 管理状态的产品语义。KAT 不遍历 Logical Plan、
冻结 `TableScan` identity、维护来源 allowlist，或统一拒绝 system catalog、table
function 和其他 DataFusion 可读来源；Runtime 启用 DataFusion URL table，使用户 SQL
可以直接读取本机文件。未来若要执行不可信 SQL，必须以独立威胁模型和完整 sandbox
另行设计。

KAT 不设置固定行数、Response 字节数、Runtime deadline 或 Host hard timeout。输出规模、
等待时间与本机资源消耗由调用方和用户负责；第一版不增加分页、流式协议、query artifact、
阈值配置、自动 `LIMIT` 或静默截断。Query 仍写入自己的 Operation log，但不创建或修改
Run、Run Output、Dataset 或持久查询状态。
