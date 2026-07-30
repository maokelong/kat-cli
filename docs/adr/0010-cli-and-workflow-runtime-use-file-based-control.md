---
status: accepted
---

# KAT Runtime IPC 使用文件交接

KAT CLI 与 Workflow Runtime 通过每次调用专属文件交换 operation-tagged JSON request 和对应的 operation-specific Runtime Response，私有 Response 与公开 KAT Response 保持分离；`query_run` 与 `test_pack` 按 ADR-0056 采用 owner-once 的可信 IPC 分工，`run_workflow` 仍按 ADR-0055 严格验收，`inspect_pack` 的封闭 operation-specific Response 也未被取代，两端原子发布使其无需跨版本协商。CLI 负责解析并 canonicalize PACK、Dataset 与 Run 等资源，Runtime 每个直接 Host 进程处理一个 request 并向随机临时路径写一次 Response；Parquet 是跨进程表数据面，子进程 stdout/stderr 只承载诊断，其合并与交付以 ADR-0057 为准，直接 Host 与私有 worker 的回收职责以 ADR-0053 为准。生产 Run 只有在进程、Response 与日志全部通过后，才由 CLI 按 ADR-0055 持久发布唯一 `manifest.json`，失败残留不形成状态；failure 与日志的归属规则由 ADR 0037 统一定义。
