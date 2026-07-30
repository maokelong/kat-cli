---
status: accepted
---

# Workflow API 与 Runtime 共用一个私有 wheel

经 ADR-0047 进一步明确，`kat/platform/workflow` 仍是 Pack Authoring API 与 Workflow Runtime 的单一源码和纯 Python wheel 构建单元，但 wheel 只提供顶层 `kat` API，当前 PACK 的 `kat.pack` 由 Runtime 动态挂载。Linux 与 Windows Builder 用标准 PEP 517/`uv` 流程把同一个 wheel 及各自锁定依赖装入 Host，使源码到各平台只有一个可验证构建 seam。该 wheel 只是 KAT 原子发布的私有中间产物，不对外分发或独立升级，也不恢复分离的 SDK 与 Runtime distributions；本文其余构建与发布决定继续有效。
