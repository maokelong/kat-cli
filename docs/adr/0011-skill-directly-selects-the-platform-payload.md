---
status: accepted
---

# Skill 直接选择平台载荷

> ADR-0062 增加的 `kat bind` 与 `kat materialize` 也通过当前 Platform Payload 使用 Bundled Python Host，并接受仅作用于本次 PACK discovery 的可重复 `--pack-dir`。无目标/Dataset inspection 不要求 Host；Dataset/Run query 在纯 Materialized 时不要求目标 PACK，但始终通过 Bundled Python Host 执行。`kat test` 仍直接接收一个精确 PACK directory。本文其余 Skill root 与 payload 选择决定继续有效。

KAT 作为一份原子 Skill 同时携带 Linux x86_64 和 Windows x86_64 的完整 Platform Payload。`SKILL.md` 在每次操作前识别当前 OS、架构和必要运行约束，然后只为 glibc 2.28 及以上的 Linux x86_64 调用 `scripts/targets/linux-x86_64/kat`，只为作为预发布候选的 Windows 10/11 x86_64 客户端调用 `scripts/targets/windows-x86_64/kat.exe`；Windows 正式支持仍以 [Issue #143](https://github.com/maokelong/kat-cli/issues/143) 的干净客户端验收为准，Windows 7/8.1、Windows Server 与其他平台明确拒绝。KAT 不再增加一个无法原生跨 Linux 与 Windows 执行的“通用启动器”，也不持久化平台选择。

KAT CLI 只根据 `current_exe()` 返回的自身位置反推出 Skill 根目录：当前可执行文件必须精确位于 `<skill>/scripts/targets/<target>/kat[.exe]`，且 `<skill>/SKILL.md` 必须是普通文件。路径层级不匹配或根级 `SKILL.md` 不成立时，Clap 已识别出的操作以 KAT Response 和同源可读诊断失败；无目标 `kat inspect` 与 `kat inspect --dataset` 不为该失败创建 Operation log。CLI 不接受 `--skill-root`，也不从当前工作目录或环境变量寻找内部资源。整个 Skill 因此可以任意移动，但不能把单个二进制复制到任意位置后继续冒充完整 Skill。

内部资源按操作需要校验，而不在每次启动时做完整性扫描。无目标 `kat inspect` 与 `kat inspect --dataset` 只需要上述最小 Skill 身份，都不检查 `agents/openai.yaml` 或 Bundled Python Host；前者把缺少 `assets/packs/` 解释为没有随 Skill 发布的 PACK，后者完全不读取 PACK 目录。后续操作只有在确实需要 Python Host 等资源时才按固定相对路径校验并使用它们。KAT 不为此增加 hash、版本 manifest、发布一致性框架、开发模式或 root override。无目标 `kat inspect`、`kat inspect --pack`、`kat run` 与 `kat test` 接受的 `--pack-dir` 只把精确 PACK directory 作为本次 PACK discovery 的显式输入，不改变 Skill 内部资源定位，也不增加父目录扫描或用户可见的来源分类。

每个目标目录本身就是 Platform Payload root。Linux Builder 必须提供根级 `kat` 与 `python/bin/python3`；Windows Builder 必须提供根级 `kat.exe` 与 `python/python.exe`，并可在载荷所需位置放置根据实际依赖计算出的 app-local DLL。Pack Authoring API、Workflow Runtime 和 Python 依赖安装在该 Host 自身的 site-packages。KAT CLI 只依赖对应私有 launcher 的固定相对路径，不读取 payload manifest、不扫描解释器、不接受环境覆盖，也不理解 Host 的其余内部目录。

产品层面，在正式支持或预发布候选的平台中，只有根级 `kat` 或 `kat.exe` 是面向用户的可执行入口；平台支持级别仍由本决策前述范围限定。CPython launcher、native extensions 与 DLL 虽然也是载荷中的可执行机器文件，但只属于私有依赖闭包，不加入 `PATH`、不由 Skill 直接调用，也不获得独立 CLI 或兼容性承诺。
