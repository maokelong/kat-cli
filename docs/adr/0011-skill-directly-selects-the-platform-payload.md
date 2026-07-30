---
status: accepted
---

# Skill 直接选择平台载荷

一份原子 KAT Skill 同时携带完整 Linux x86_64 与 Windows x86_64 Platform Payload，由 `SKILL.md` 在每次操作前直接选择受支持目标并拒绝其他环境，不增加通用启动器或持久平台选择。KAT CLI 只能从 `current_exe()` 的固定 Skill 内层级定位根目录与内部资源，不接受 cwd、环境变量或参数覆盖，并按当前操作惰性校验资源；因此整份 Skill 可以移动，脱离它复制的单个二进制不能冒充完整产品。每个 Payload 只固定 KAT CLI 与私有 Bundled Python launcher 的相对入口，Host、native extensions 和 DLL 均属于 Builder 拥有的依赖闭包，`kat` 或 `kat.exe` 是唯一公开且受支持的可执行入口。
