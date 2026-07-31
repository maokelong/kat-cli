---
name: kat
description: 分析本地 Hitrace、已有 KAT Dataset 或既有 Run，并理解、创建、修改、验证或诊断 KAT PACK。
---

# KAT

KAT 是唯一面向用户的产品入口。用户用自然语言说明目标，不需要选择 `import`、`inspect`、`run`、`query` 或 `test` 等内部操作。

## 你可以请求什么

### 分析数据

一次任务从一个 Source、已有 Dataset 或既有 Run 加上要回答的问题开始：

- 提供本地 `.htrace` 和问题，KAT 导入后完成分析。
- 提供已有 Dataset 和问题，KAT 直接分析。
- 提供已有 Run 和追问，KAT 只查询既有输出，不重新执行。

KAT 只在缺少继续所需的事实，或多个候选会导向实质不同结论时追问。你可以选填 Dataset、PACK、Workflow 或 External PACK directory 来覆盖自动选择，但这不是正常使用的前提。

分析结果包含直接结论、少量可追溯证据、适用范围或不确定性，以及可选的下一步探索方向。当前可分析主题由运行时发现的 PACK 和 Workflow 决定，不维护静态能力清单。Trace Streamer 仅用于内部机制验证，不用于正常分析。

示例：

- “分析这个 `.htrace`，找出线程 CPU 时间异常的原因。”
- “基于这个 Dataset，判断调度延迟是否集中在特定 CPU。”
- “继续查看这个 Run 中异常线程的明细。”

### 创作或维护 PACK

你可以要求 KAT 理解已有 PACK、新建或修改 PACK、校验或测试 PACK，或诊断失败并提出或实施最小修复。

理解、校验、测试和解释失败默认只读。只有明确要求创建、修改或修复时，KAT 才写入你指定的 PACK 源码；写入交付会说明变更、受影响文件和实际验证证据。

示例：

- “解释这个 PACK 能解决什么问题，并指出它的限制。”
- “给这个 PACK 增加线程 CPU 时间分析。”
- “修复这个 PACK 的失败测试，只改必要代码。”

## 数据放在哪里

KAT 在本机读取 Source 而不改写它，并在 KAT Data Home 创建 Dataset、Run 和日志等结果。Bundled PACK 随 Skill 交付；External PACK 及其测试是受信任的本地代码，不在安全沙箱中运行。

### 默认位置与更换方式

Data Home 的默认配置文件位于 Linux 的 `$XDG_DATA_HOME/kat/config.json`（未设置时为 `$HOME/.local/share/kat/config.json`），或 Windows 的 `%APPDATA%\KAT\data\config.json`。

首次需要写入 KAT 状态时，Skill 会提醒你当前平台的默认位置，并询问是否要更换 Data Home：

- 不更换：不编辑配置，也不改动环境变量。
- 更换：提供一个已存在、可访问的绝对目录。Skill 会校验路径，更新上述默认位置的 `config.json`，并让本次 KAT 操作立即使用新目录。

路径中的 `~`、`%USERPROFILE%` 和 `$HOME` 等缩写不会展开。无效路径或损坏配置会停止操作，不会覆盖原文件或擅自改用其他目录。

## Agent 执行顺序

### 1. 每次操作前选择平台载荷

先把本 `SKILL.md` 的父目录解析为绝对 `<skill-root>`，再重新检查当前主机：

- Linux：读取 `uname -m` 与 `getconf GNU_LIBC_VERSION`。仅支持 glibc 2.28 或更高版本的 x86_64，执行 `<skill-root>/scripts/targets/linux-x86_64/kat`。
- Windows：读取原生架构、系统版本与 `Win32_OperatingSystem.ProductType`。仅支持 Windows 10/11 x86_64 客户端（`ProductType=1`），执行 `<skill-root>/scripts/targets/windows-x86_64/kat.exe`；拒绝 Windows Server、Windows 7/8.1。

拒绝其他系统、架构、libc 或版本；所选 Payload 缺失时也拒绝，Linux 还需确认可执行位。始终使用这些绝对路径，不持久化选择，不搜索 `PATH`，不回退到系统 Python 或系统 `kat`。

### 2. 在首次状态写入前确认 Data Home

首次需要写入 KAT 状态时，先展示当前平台默认 Data Home 与其 `config.json` 路径，并询问用户是否要更换。这个问题每次对话只问一次；用户没有要求更换时，不编辑配置也不设置、清空或猜测 `KAT_DATA_HOME`。

用户明确要求更换且提供路径后：

1. 校验路径是已存在、可访问的绝对目录；不展开路径缩写，非法路径停止并说明原因。
2. 读取平台默认位置的 `config.json`；文件不存在时创建父目录与新的 JSON object，已存在时必须是可解析 JSON object，并原样保留未知字段。不要读取 Skill 根目录的同名文件。
3. 将 `kat_data_home` 更新为已规范化的目录并写回同一配置文件。配置无法读取、解析或写入时停止，不覆盖损坏文件，也不调用 KAT。
4. 仅为用户当前请求的 KAT 进程设置同一 `KAT_DATA_HOME`，确保本次立即使用新目录；不修改用户的全局环境。后续没有该环境变量的 KAT 进程由配置文件选择。

KAT 依次选择非空 `KAT_DATA_HOME`、平台默认 KAT 数据目录中的非空 `config.json.kat_data_home`、平台默认 Data Home。非空 `KAT_DATA_HOME` 先校验：有效时直接选中且不读取 `config.json`，无效时操作失败且不回退；只有该变量缺失或为空时才读取配置。环境变量或配置中的非空 Data Home 必须是已存在、可访问的绝对目录；无效的已选值会使操作失败，不回退到较低优先级来源。配置文件缺失、字段缺失或空字符串表示未设置。

`KAT_DATA_HOME` 只影响启动它的单次进程，不改变 `config.json` 的读取位置。KAT CLI 仍是 Data Home 选择和运行时失败语义的唯一权威。Data Home 选择失败时，根据 KAT Response 的 Diagnostic 交付，不改写已选择的配置或改用其他目录重试。

### 3. 按任务类型加载流程

先把请求分类为“分析数据”或“创作/维护 PACK”，再加载对应 reference：

- 分析数据：读取 [analysis-flow.md](references/analysis-flow.md)。
- 创作或维护 PACK：读取 [pack-authoring-flow.md](references/pack-authoring-flow.md)。
- 在向用户交付前：读取 [result-contract.md](references/result-contract.md)。

无匹配的 PACK/Workflow 是分析受阻，不自动切换为新建 PACK 或写入源码；可以把新建或扩展 PACK 作为需用户确认的下一步。

### 4. 以 KAT Response 为事实来源

KAT Response 是操作成功、失败和可用产物的唯一权威事实。Operation log、pytest terminal report 与 PACK Test Report 只用于解释、诊断和引用证据，不得通过解析人类文本反向推断操作状态。
