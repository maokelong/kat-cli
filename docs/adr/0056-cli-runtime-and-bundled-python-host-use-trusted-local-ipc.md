---
status: accepted
---

# CLI、Workflow Runtime 与 Bundled Python Host 的可信本地 IPC

本决定取代 ADR-0008、ADR-0010、ADR-0019 与 ADR-0038 中仅针对 `query_run` 的 Output ID、固定查询限制、严格私有 DTO、CLI 平行复核和数据来源门禁条款；不改变这些 ADR 的其他决定、ADR-0055 的 Run publication，也不取代 ADR-0016 的 pytest、失败投影与 PACK Test Report 语义，`test_pack` 只采用本文的信任分工。KAT CLI、Bundled Python Host 与 Workflow Runtime 是同版本原子发布的可信单元，私有 IPC 不是安全边界、公共协议或跨版本接口；CLI 拥有公共输入、选择和证据发布，Runtime 拥有 PACK/pytest/Workflow 执行、Arrow 领域结果与诊断，Host 只承载进程，事实由 owner 校验一次后接收方仅验证传输并构造 typed value，从而避免两种语言维护平行语义，且 `run_workflow` 仍遵循 ADR-0055。对 `query_run`，Runtime 唯一负责 Arrow 到公开 Query Result 的投影，CLI 只附加自己拥有的当前 Dataset 状态；用户 SQL 作为可信本地只读输入可读取本机来源，但不能修改 KAT 状态，KAT 不再设置来源 allowlist、固定输出限制、deadline、分页或静默截断，资源消耗由调用方负责且查询不产生持久状态。
