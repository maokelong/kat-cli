# KAT Skill 任务契约设计

## 状态

已实施。本文记录首个文档切片的任务契约、内容组织和验收场景。

## 要解决的问题

当前 `$kat` 的自然语言契约与任务流程说明不足。用户和 Agent 不容易判断：KAT 能完成什么任务、需要提供哪些事实、何时会被追问，以及完成、失败或等待输入时会得到什么。

## 非目标

1. 不把 `import`、`inspect`、`run`、`query`、`test` 暴露为多个独立 Skill。
2. 不把 CLI 参数或 KAT Response 的实现细节写进用户层。
3. 不维护会随 Bundled 或 External PACK 变化而过期的静态 PACK/Workflow 清单。
4. 不把 Skill 变成远程服务、安装器或 External PACK 的安全沙箱。

## 已确认的任务契约

`$kat` 仍是唯一入口，只有两类一级任务：

1. 分析数据。
2. 创作或维护 PACK。

能力说明、最小澄清、排障和下一步建议嵌入这两类任务，不成为第三类 Skill。

### 分析数据

分析任务接受三种起点：

1. Source 加问题：导入后选择 PACK/Workflow 并执行。
2. 已有 Dataset 加问题：跳过导入，选择并执行。
3. 已有 Run 加追问：只对既有输出做有界查询，不重新执行。

一次分析任务只围绕一个 Source、一个 Dataset 或一个 Run；不隐式拼接多个输入或推断跨输入比较语义。需要这种能力时，由明确的 PACK 设计承担。

Skill 仅在缺少继续所需的关键事实，或候选选择会导向实质不同结论时追问。其他选择采用可说明的默认值。

成功的 Analysis Result 稳定包含：直接结论、少量可追溯证据、适用范围或不确定性，以及可选的下一步探索方向。它不直接转发完整表、日志或 CLI JSON。

已有 Dataset、目标 PACK/Workflow 和 External PACK directory 是可选的高级控制，用于覆盖自动选择；普通用户无需了解它们的内部 CLI 句法。

如果当前 Dataset 与可发现的 PACK/Workflow 没有可执行匹配，分析任务以受阻结果交付已检查的输入与能力边界。它可以建议新建或扩展 PACK 作为下一步，但不得自动切换到作者工作流或写入源码。

### 创作或维护 PACK

同一个作者工作流支持四种意图：理解已有 PACK、新建或修改 PACK、校验或测试 PACK、诊断失败并提出或实施最小修复。

新建 PACK 不以 Issue 或 SDD 为 KAT 任务契约的前置门；这些协作约束如有需要，属于执行所在仓库的外部治理规则，而不是 KAT 产品工作流。

理解、校验、测试和解释失败默认只读。只有用户明确要求创建、修改或修复时，Skill 才写入明确指定的 PACK 源码位置；任何写入交付都说明变更、受影响文件和实际验证证据。

理解已有 PACK 的只读交付包括：它解决的问题与可用 Workflow、每个 Workflow 所需的 Dataset facts 或参数、现有测试或验证证据、明确限制与下一步；不复述目录、manifest 或源码。

### 共同结果状态

每次任务只以三种状态之一交付：已完成、需要补充信息、执行失败或受阻。后两者必须说明受阻阶段、已经验证的证据、具体原因和最小下一步；不得把部分内部进展包装成完成。

### 能力、数据与信任边界

Skill 静态说明稳定的任务类别和正常输入边界。当前正常分析输入是 `.htrace`；Trace Streamer 只用于内部机制验证，不属于正常分析。具体可用 PACK、Workflow 和分析主题由执行时 discovery 获取。

KAT 在本机读取 Source 而不改写它，并在 KAT Data Home 创建 Dataset、Run 和日志等结果。Bundled PACK 随 Skill 交付；External PACK 及其测试是受信任的本地代码，不在安全沙箱中执行。

Data Home 服从 [PR #177](https://github.com/maokelong/kat-rs/pull/177) 的选择契约：非空 `KAT_DATA_HOME` 优先于平台默认 KAT 数据目录中的非空 `config.json.kat_data_home`，再回退平台默认目录。非空覆盖必须是可访问绝对目录，失败不回退。首次需要写入 KAT 状态时，Skill 提醒用户平台默认位置并询问是否更换；用户明确给出路径后，Skill 校验路径，保留未知 JSON 字段地更新平台默认位置的 `config.json`，并只为当前 KAT 进程设置同一路径的环境变量。CLI 本身仍不创建或写入配置。

## 内容组织

一份任务契约服务两个受众：

```text
SKILL.md
  用户层：能力、输入、交付、边界、目标式示例
  Agent 层：意图路由、最小澄清、结果状态、参考文档选择
references/
  analysis-flow.md
  pack-authoring-flow.md
  result-contract.md
```

`SKILL.md` 是薄入口，只保留每次任务都需要的规则。三个 reference 分别承载三种分析起点与证据规则、PACK 作者流与写入边界、以及完成/等待输入/失败的交付格式。两层必须复用同一术语和承诺。

用户层为两条主线提供目标式示例，例如“分析这个 `.htrace`，找出线程 CPU 时间异常的原因”或“修复这个 PACK 的失败测试，只改必要代码”；示例不是固定命令模板。

Agent 以 KAT Response 作为操作是否成功、失败和产物是否可用的唯一权威事实。Operation log、pytest terminal report 与 PACK Test Report 只用来解释、诊断或引用证据，不能经解析人类文本反向推断操作状态。

Agent 以如下最小事实检查点推进任务，并只保留完成下一步所需的字段：

| 阶段 | 权威事实 |
| --- | --- |
| Source 导入 | 成功返回的 Dataset `path` |
| Dataset 理解 | `path`、tables 与 schema |
| 自动选择 | 可发现 PACK 概要；候选 PACK 的 Workflow、`required_tables`、参数 |
| Workflow 执行 | `run_id`、输出名称、列与行数 |
| Run 追问 | 有界 Query 的列与行 |
| PACK 校验/测试 | inspection 结果；测试 `summary`、`test_report_path` 与可选 `log_path` |

三个 reference 按可执行生命周期编排，而不是按概念堆叠：

1. `analysis-flow.md`：识别 Source/Dataset/Run 起点，获取必要事实，自动选择或最小澄清，执行或有界追问，形成证据与结果。
2. `pack-authoring-flow.md`：定位 PACK，先 inspect，执行只读理解或已授权变更，inspect/test，诊断与交付。
3. `result-contract.md`：已完成、需要补充信息、执行失败/受阻三种状态的固定交付模板和证据引用规则。

每个阶段必须说明需读取的 KAT Response 事实、成功后的下一步和失败后的下一步。

## 验证

实现以六个契约场景验收，而不是只检查 `SKILL.md` 能否被加载：

1. Source → 导入、自动选择、执行，并交付带证据的 Analysis Result。
2. 多个实质不同候选 → 只提出一个最小必要澄清问题。
3. Run 追问 → 只查询已有 Run，不重新执行 Workflow。
4. 无匹配能力 → 如实交付受阻结果，不自动写 PACK。
5. 理解 PACK → 全程只读，交付能力、前提、验证证据与限制。
6. 明确修复 PACK → 只作必要改动，并交付 inspect/test 证据。

## 最小实现切片

只新增或重写 KAT Skill 的任务说明与上述三个 reference，不改变 KAT CLI、Payload、PACK Runtime 或既有领域语义。实现 PR 以本节六个场景提供可复核证据。
