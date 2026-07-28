---
status: accepted
---

# Runtime 诊断输出不保留标准流身份

Runtime Response 继续只通过 CLI 指定的私有文件交接；Runtime 及其子进程的 stdout/stderr 只承载不可信诊断内容，不参与结果协议。CLI 可以在进程边界先合并两条标准流，也可以分别捕获后再组合其文本投影；消费者不得依赖原始流身份或精确交错顺序。`kat test` 为满足实时终端报告，将 stdout/stderr 合并为一条诊断字节流，只维护一份跨 chunk 的 ANSI 清理、UTF-8 与控制字符投影状态，并只为整条流补一个最终换行；同一投影同时写入 stderr 与 Operation log。读取、日志写入或终端投影失败时，CLI 仍必须终止并回收 Runtime，拒绝接受 Runtime Response。本 ADR 仅取代 ADR 0010 中要求分别投影并排空两条标准流的部分，不改变其余 IPC 与可信边界。
