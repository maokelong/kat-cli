---
status: accepted
---

# Runtime 诊断输出不保留标准流身份

Runtime Response 仍只通过 CLI 指定的私有文件交接，stdout/stderr 只承载不可信诊断；CLI 可合并后投影，消费者不得依赖原始流身份或精确交错顺序。`kat test` 将两条流合为一份跨 chunk 规范化的诊断，同时写入 stderr 与 Operation log；读取或投影失败时必须终止并回收 Runtime，拒绝 Response。本决定仅取代 ADR-0010 中分别投影并排空两条标准流的要求，不改变其余 IPC 与可信边界。
