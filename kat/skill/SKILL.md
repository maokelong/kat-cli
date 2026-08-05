---
name: kat
description: 分析已有 KAT Dataset 或 Run，在明确的预发布边界下验证 Trace Streamer 数据，并理解、创建、修改、验证或诊断 KAT PACK。
---

# KAT

KAT 是唯一面向用户的产品入口。用户用自然语言说明目标，不需要选择 `import`、`inspect`、`run`、`query` 或 `test` 等内部操作。

## 你可以请求什么

### 分析数据

一次任务从一个预发布 Source、已有 Dataset，或既有 Run 及其 Run Output 元数据，加上要回答的问题开始：

- 明确要求试用本地 Trace Streamer SQLite 和问题，KAT 可以通过当前预发布 PACK 完成验证性分析。
- 提供已有 Dataset 和问题，KAT 直接分析。
- 提供已有 Run、输出名称和 columns 及追问，KAT 只查询既有输出，不重新执行。

KAT 只在缺少继续所需的事实，或多个候选会导向实质不同结论时追问。你可以选填 Dataset、PACK、Workflow 或 External PACK directory 来覆盖自动选择，但这不是正常使用的前提。

分析结果包含直接结论、少量可追溯证据、适用范围或不确定性，以及可选的下一步探索方向。当前可分析主题由运行时发现的 PACK 和 Workflow 决定，不维护静态能力清单。Trace Streamer 入口及依赖它的 OpenHarmony PACK 均为 Deprecated 预发布能力，只在用户明确要求试用或验证时使用，不承诺稳定 Schema、生产兼容性或迁移路径。`.htrace` 的长期 Bundled Workflow 尚未就绪；收到 `.htrace` 分析请求时如实说明能力边界，不把它改写为 Trace Streamer 结果。

示例：

- “试用这个 Trace Streamer SQLite，查看线程 CPU 时间主要分布在哪些 CPU。”
- “基于这个 Dataset，判断调度延迟是否集中在特定 CPU。”
- “继续查看这个 Run 中异常线程的明细。”

### 创作或维护 PACK

你可以要求 KAT 理解已有 PACK、新建或修改 PACK、校验或测试 PACK，或诊断失败并提出或实施最小修复。

理解、校验、测试和解释失败默认只读。只有明确要求创建、修改或修复时，KAT 才写入你指定的 PACK 源码；写入交付会说明变更、受影响文件和实际验证证据。

示例：

- “解释这个 PACK 能解决什么问题，并指出它的限制。”
- “给这个 PACK 增加线程 CPU 时间分析。”
- “修复这个 PACK 的失败测试，只改必要代码。”

### 直接使用命令（高级用户与 Agent）

用户仍可只用自然语言提出目标。需要直接调用 CLI 时，先看 [命令速查](references/command-reference.md)：其中给出了每个命令的调用模板、适用时机、成功 Response 中可继续使用的字段，以及危险参数的边界。Agent 必须按速查调用，不猜测参数或解析人类可读终端文本。

## 数据放在哪里

KAT 在本机读取 Source，并在 KAT Data Home 创建 Dataset、Run 和日志等结果。KAT 不直接改写 Source 内容；但显式指定 Dataset 目标并使用 `--overwrite-dataset` 时，会永久清空解析后的整个目标，且不检测 Source 是否位于目标内。Agent 调用前必须让用户确认 Source 与 Dataset 目标不重叠。Bundled PACK 随 Skill 交付；External PACK 及其测试是受信任的本地代码，不在安全沙箱中运行。

### 默认位置与更换方式

Data Home 的默认配置文件位于 Linux 的 `$XDG_DATA_HOME/kat/config.json`（未设置时为 `$HOME/.local/share/kat/config.json`），或 Windows 的 `%APPDATA%\KAT\data\config.json`。它是由用户维护的 KAT 私有应用配置；KAT CLI 和本 Skill 都不创建或写入它。

首次需要写入 KAT 状态时，Skill 会提醒你当前平台的默认位置，并询问是否要更换 Data Home：

- 不更换：直接使用默认位置，不编辑配置，也不改动环境变量。
- 更换：Skill 展示当前平台的准确配置路径和以下手工修改内容，由你自行创建或编辑该文件；你确认完成后，Skill 再继续调用 KAT。

```json
{
  "kat_data_home": "<已存在、可访问的绝对目录>"
}
```

编辑已有 JSON object 时只增加或更新 `kat_data_home`，保留其他字段。路径中的 `~`、`%USERPROFILE%` 和 `$HOME` 等缩写不会展开。KAT 会拒绝无效路径或损坏配置，不会擅自改用其他目录。

## Agent 执行顺序

### 1. 每次操作前选择平台载荷

先把本 `SKILL.md` 的父目录解析为绝对 `<skill-root>`，再重新检查当前主机：

- Linux：读取 `uname -m` 与 `getconf GNU_LIBC_VERSION`。仅支持 glibc 2.28 或更高版本的 x86_64，执行 `<skill-root>/scripts/targets/linux-x86_64/kat`。
- Windows：读取原生架构、系统版本与 `Win32_OperatingSystem.ProductType`。Windows 10/11 x86_64 客户端（`ProductType=1`）是预发布候选目标，执行 `<skill-root>/scripts/targets/windows-x86_64/kat.exe`；正式支持仍需完成 [Issue #143](https://github.com/maokelong/kat-rs/issues/143) 的干净客户端验收。拒绝 Windows Server、Windows 7/8.1。

拒绝其他系统、架构、libc 或版本；所选 Payload 缺失时也拒绝，Linux 还需确认可执行位。始终使用这些绝对路径，不持久化选择，不搜索 `PATH`，不回退到系统 Python 或系统 `kat`。

### 2. 在首次状态写入前确认 Data Home

首次需要写入 KAT 状态时，展示当前平台默认 Data Home 与 `config.json` 路径，并询问用户是否要更换。这个问题每次对话只问一次；用户接受默认位置时直接继续，不编辑配置，也不设置、清空或猜测 `KAT_DATA_HOME`。

用户要求更换时，获取一个已存在、可访问、可规范化的绝对目标目录，然后展示平台默认 `config.json` 的绝对路径以及只更新 `kat_data_home`、保留其他字段的 JSON 示例。不要创建目录、读取或修改配置，也不要替用户设置环境变量。等待用户确认已经手工完成修改；确认前不调用 KAT，确认后由 KAT CLI 按自身配置规则验证和选择 Data Home。

KAT 依次选择非空 `KAT_DATA_HOME`、平台默认 KAT 数据目录中的非空 `config.json.kat_data_home`、平台默认 Data Home。所有已经提供的来源都必须先通过校验：即使最终由环境变量覆盖，已存在的 `config.json` 也必须可读、是有效 UTF-8 和合法 JSON object，且 `kat_data_home` 必须是字符串；损坏配置会使操作失败。只有最终选中的非空 Data Home 才检查它是否为已存在、可访问的绝对目录，被更高优先级覆盖的路径字符串不访问文件系统。无效的已选值会使操作失败，不回退到较低优先级来源。配置文件缺失、字段缺失或空字符串表示未设置。

`KAT_DATA_HOME` 只影响启动它的单次子进程，不改变 `config.json` 的读取位置。KAT CLI 是 Data Home 选择和运行时失败语义的唯一权威。Data Home 选择失败时，根据 KAT Response 的 Diagnostic 交付，不改写配置、清空环境变量或改用其他目录重试。

### 3. 先查命令速查，再按任务类型加载流程

先读取 [command-reference.md](references/command-reference.md)，再把请求分类为“分析数据”或“创作/维护 PACK”，并加载对应 reference：

- 每次调用 CLI：按 [command-reference.md](references/command-reference.md) 选择精确命令、参数和 Response 字段。
- 分析数据：读取 [analysis-flow.md](references/analysis-flow.md)。
- 创作或维护 PACK：读取 [pack-authoring-flow.md](references/pack-authoring-flow.md)。
- 在向用户交付前：读取 [result-contract.md](references/result-contract.md)。

无匹配的 PACK/Workflow 是分析受阻，不自动切换为新建 PACK 或写入源码；可以把新建或扩展 PACK 作为需用户确认的下一步。

### 4. 以 KAT Response 为事实来源

KAT Response 是操作成功、失败和可用产物的唯一权威事实。Operation log、pytest terminal report 与 PACK Test Report 只用于解释、诊断和引用证据，不得通过解析人类文本反向推断操作状态。
