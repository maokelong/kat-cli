---
status: accepted
---

# 受支持的执行强制使用 Bundled Python Host

KAT Skill 始终从当前 Platform Payload 的固定相对路径以 isolated mode 启动 Bundled Python Host，忽略用户 Python 环境且不提供 fallback 或解释器覆盖；这是保证 Pack Authoring API、依赖集合和行为一致的可移植性边界，不是针对本机用户的安全沙箱。Linux 与 Windows 载荷各自离线携带固定、可重定位的 standard-GIL CPython 和完整依赖闭包，Windows 同时携带所需 app-local VC Runtime；KAT 不使用指向系统解释器的 venv、冻结式应用或首次运行解包方案，因为 External PACK 需要完整标准库、动态 import 和只读载荷。Pack Authoring API 与 Workflow Runtime 由同一内部构建单元生成并安装为一个私有 wheel，不形成独立 SDK、distribution 或兼容承诺，`kat` 或 `kat.exe` 是唯一受支持的公开可执行入口。
