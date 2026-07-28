---
status: accepted
---

# CLI、Workflow Runtime 与 Bundled Python Host 的可信本地 IPC

本决定取代 ADR-0008、ADR-0010 与 ADR-0019 中仅针对 `query_run` 的 Output ID、
固定查询限制、严格私有 DTO 和数据来源门禁条款。Run publication 已按 ADR-0055
使用 `(run_id, output_name)` 作为完整公开引用，且不维护第二套物理名称；因此 CLI
从最终 Manifest 取得全部 Output names，Runtime 按 canonical Run path 与名称确定性
打开 `outputs/<output-name>.parquet`。这项取代不改变上述 ADR 的其他决定，也不改变
ADR-0055 归属的 Run publication。`test_pack` 同样采用本文的私有 IPC 信任分工，但本文不取代
ADR-0016 对 pytest、失败投影或 PACK Test Report 的行为语义；它只规定这些既有语义在同版本可信单元
内由哪一方拥有并消费。

## `test_pack` 的行为边界

ADR-0016 唯一规定 `test_pack` 的 pytest ExitCode、`kat_run` 已知 Workflow 失败投影、terminal report 与
PACK Test Report 语义。本 ADR 不复制这些规则，也不把 CLI 对 Runtime 已确定的测试结果、summary 或
diagnostic 再校验一次。

在本文模型中，Runtime 形成并拥有上述 pytest 结果与 diagnostic；CLI 只拥有进程/传输边界，以及其
自己分配的 Operation log 和 Test Report 路径是否可交付。因而 Runtime failure 由 CLI 原样投影，只有
Host 未完成 IPC 或 CLI 自己的证据交付失败才由 CLI 形成新的失败。

## 可信发布单元与私有 IPC

KAT CLI、Bundled Python Host 与 Workflow Runtime 是同一 KAT 版本构建并原子发布的可信执行单元。
受支持的 CLI 只从当前 Platform Payload 启动其相邻的 Bundled Python Host；Host 只从自身安装的
私有 Workflow wheel 加载 Runtime。用户、Skill、模型与 PACK 均不直接构造、修改或复用私有
Runtime IPC。

因此，私有 IPC 不是安全边界、公开协议或跨版本兼容接口。用户自行混装 CLI、Host、wheel 或篡改
控制文件的后果不属于受支持范围；KAT 不为这类情形维护协议版本协商、未知字段兼容、平行语义校验
或降级路径。

已经采用本模型的私有操作中，每项业务事实必须只有一个语义 owner：

- CLI 拥有公共参数解析、PACK 发现与选择、canonical 路径确定、Dataset 解析、用户可见输入校验，
  以及最终 KAT Response、Operation log 和 CLI 分配证据文件的发布。
- Workflow Runtime 拥有 PACK 加载、Workflow 与 pytest 执行、DataFusion/Arrow 领域处理、Runtime
  结果与 Runtime diagnostic 的构造，以及 Arrow 到公开结果值的唯一投影。
- Bundled Python Host 只承载 Runtime 进程；它不构成第二个产品层或独立兼容面。

当前 `query_run` 与 `test_pack` 已采用本模型。`run_workflow` 仍遵循 ADR-0055 明确规定的
Response acceptance；在专门的迁移决定取代该规则前，本 ADR 不以通用表述改变其现有验收职责。

owner 在将事实写入已采用本模型的私有 request 或 result 前完成一次业务校验。接收方只将同版本 JSON 构造成内部
typed value 并消费该事实，不重复验证已经由 owner 确定的 UUID、canonical path、PACK identity、
字段全集、Dataset 表集合、Output name、selector、Arrow 类型、行/列结构、summary 或 diagnostic
语义。

已采用本模型的操作中，CLI 对 Runtime 回包只负责进程与传输边界：确认 Host 已退出、Response 文件存在且可读取、JSON 完整
且可解码。CLI 可以检查自己将要公开的日志、报告或其他 CLI-owned 证据是否实际可交付，但不得以此
重新裁决 Runtime 已生成的领域结果或失败语义。

Runtime 因实际执行产生的失败，例如 PACK 无法导入、pytest 收集失败、Workflow 执行失败、Arrow
投影失败，属于 Runtime 的执行结果；它们不是对 CLI 已确认 request 事实的第二次校验。CLI 将该结果
移动到最终公开 Response，除 CLI 自己后续发布门失败外，不改写、不合并、不重新解释该 Runtime 结论。

内部一致性由 typed construction 和真实 CLI 到 Bundled Host 的纵向契约测试证明，而不是由两种语言维护
平行的防御矩阵。每个受支持私有操作至少验证：从同一候选提交构建 CLI 与 Workflow wheel，将 wheel
安装到 staged Host，由 staged CLI 发起实际操作，并覆盖成功、Runtime 执行失败与传输失败路径。

## `query_run` 专属约束

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
