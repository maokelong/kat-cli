# PACK 创作与维护流程

## 1. 定位并先检查 PACK

用户指定已有 PACK 时，先通过私有 `kat inspect --pack` 和需要时的精确 `--pack-dir` 定位它。只在成功 Response 中读取 PACK identity、Workflow、Required tables、参数和验证入口；inspection 失败时停止，保留 Diagnostic 和可用 `log_path`，按 [result-contract.md](result-contract.md) 交付。不要自行扫描 manifest、猜测 PACK 目录或加载 PACK Python 代码来替代 inspection。

新建 PACK 没有 KAT 任务契约内的 Issue 或 SDD 前置门；执行所在仓库的协作规范仍独立适用。

## 2. 只读理解、校验和测试

对于“理解 PACK”，交付它解决的问题与 Workflow、每个 Workflow 所需的 Dataset facts 或参数、已有测试或验证证据、明确限制与下一步；不要复述目录、manifest 或源码。

对于校验或测试，先 inspection，再执行私有 `kat test`。只从 success Response 的 `summary`，以及存在时的 `test_report_path` 与 `log_path` 判断和引用测试证据。test failure 时停止成功路径，保留 failure Response 的 Diagnostic、可用 `test_report_path` 与 `log_path`，按 [result-contract.md](result-contract.md) 交付。pytest terminal report 用于定位失败 node ID 和解释断言，但不推断 KAT 操作状态。

## 3. 已授权的变更或修复

只有用户明确要求创建、修改或修复时才能写入指定 PACK 源码。变更前先确认目标 PACK 位置与用户目标；保持最小切片，不修改 Skill、Platform Payload 或无关 PACK。

每次写入后：

1. 重新 inspection，确认生产 Interface 与 Required tables。
2. 运行适用的 `kat test`；失败时使用报告和日志诊断，但不把失败说成完成。
3. 交付变更摘要、受影响文件、实际验证证据和仍存限制。

“诊断失败”本身不授权修复。无法在现有授权和事实下继续时，按 [result-contract.md](result-contract.md) 交付最小下一步。
