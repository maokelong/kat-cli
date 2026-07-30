---
status: accepted
---

# Workflow Runtime 进程只处理一个 tagged request

Runtime Request 是私有的 operation-tagged union，Runtime Response 由已知 Request 选择对应类型而不回显 tag，并与公开 KAT Response 保持分离；`query_run` 与 `test_pack` 的事实所有权和传输校验遵循 ADR-0056，`run_workflow` 仍遵循 ADR-0055 的严格 Response acceptance，`inspect_pack` 保持封闭 operation-specific 类型，Diagnostic 的请求事实边界遵循 ADR 0037。CLI 只生成 `inspect_pack`、`run_workflow`、`query_run` 和 `test_pack` 四类请求，按 ADR-0047 为需要 PACK 的操作显式传递业务 name 与挂载 `kat.pack` 的 canonical path，并先解析所需资源；每个 Bundled Python Host 只处理一个请求、写一次私有临时 Response 后退出，不消费多请求或提供泛化运行模式，直接 Host 与私有 worker 的生命周期按 ADR-0053 分层拥有。CLI 独占最终 KAT status、JUnit 路径和 ADR-0055 定义的 Run Manifest 发布，Runtime 不写生产 `manifest.json`；两端随同一原子 KAT 包发布，因此不增加 schema version、协商、迁移或兼容 fallback。
