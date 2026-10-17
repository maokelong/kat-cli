---
name: kat-author
description: 理解、创建、修改和验证 KAT PACK 的独立知识、Provider、Workflow 与领域公共库。按具体创作目标选择流程。
---

# KAT Author

**必须按当前创作目标遵循以下流程。** 只读理解、检查、测试和诊断不自动授权修改；没有匹配分析能力也不构成创作授权。只实现用户授权的最小 PACK 变更，不让纯知识或空骨架任务进入完整程序开发。

调用 KAT 前读取相邻 [共享执行合同](../kat/references/command-reference.md)，从本文件绝对位置解析 `../kat/`，取得同套部署的 CLI、Data Home 与返回规则，不使用工作目录或其他版本。以下 `kat` 使用该 CLI 的绝对路径。

本目录为 `<author-root>`，拥有[骨架脚本](scripts/scaffold_pack.py)与[程序示例](references/examples/dataprovider-pack/README.md)。资源从本文件位置解析；用户 PACK 从已明确的目标目录解析。缺少目录时询问精确位置，不全局搜索。

## 1. 明确目标与选择路径

| 用户目标 | 使用流程 |
| --- | --- |
| 只读理解、检查或诊断 | 按对象 inspection 与已有材料回答，不进入写入和安装 |
| 只创建骨架 | 执行下一节，不要求业务能力、SDK API 或 pytest |
| 固化或修改独立知识 | 进入知识创作，不要求 Python 实现与 SDK API |
| 新增或修改 Workflow、Provider、公共函数 | 先做程序复用检查，再实现与验证 |
| 仅改文案、格式或不影响能力的测试 | 按变更面维护与验证，可以跳过程序复用检查 |

已有 PACK 直接用 `kat inspect --pack-dir D` 验证 manifest，再按目标检查；只有名称时先用 `kat inspect` 定位公开能力，需要外部位置时取得目录。后续相关调用持续携带精确 `--pack-dir D`，不能把 PACK 集合父目录当作单个 PACK。

## 2. 创建 PACK 骨架

先确定小写 ASCII kebab-case 的 name、面向用户的 title、领域边界 description、真实 owner 和 PACK 集合目录。名称可按领域生成；不能可靠推断 owner 或目录时只询问缺项，不用占位团队或路径。

用可用的宿主 Python 执行纯标准库脚本；KAT 命令仍使用共享载荷与私有 Host：

```text
python <author-root>/scripts/scaffold_pack.py --packs-dir <集合绝对目录> --name <name> --title <title> --description <description> --owner <owner>
kat inspect --pack-dir <生成的pack_directory>
```

最终位置固定为 `<packs-dir>/<name>`，脚本拒绝覆盖。只创建 manifest、README 和标准创作目录，不生成假 Workflow、Provider 或测试。成功 JSON 的 `pack_directory` 与 `entries` 用于展示实际完整目录树及用途。按用户同时要求的能力继续创作；空列表仅证明骨架可被识别。

## 3. 独立知识创作

先明确适用对象、触发条件、输入、结果与限制。无需程序的方法写为 `skills/<entry>/SKILL.md`；需要程序稳定完成的部分仍用 Workflow，不写空函数制造 Run。纯知识 PACK 最少只需合法 `pack.toml` 和知识入口，不要求程序目录或 pytest。

知识入口 YAML 只允许必填字符串 `name,description`；name 与目录一致，遵循 PACK 小写 ASCII kebab-case 与 Windows 保留名规则，头部和正文均非空。不增加执行器、模型、Workflow 绑定或依赖元数据。

正文写明方法与边界，引用邻近 references 或 PACK 内共享 knowledge；链接相对包含它的文件。参考随包的[最小知识入口](references/examples/knowledge-pack/skills/evidence-review/SKILL.md)与[清单](references/examples/knowledge-pack/pack.toml)。知识不能授予新的外部访问、执行、安装或写入权限。

验证本次入口及实际引用：

```text
kat inspect knowledge --pack P --pack-dir D
kat inspect knowledge --pack P --entry E --pack-dir D
kat inspect knowledge --pack P --path F --pack-dir D
```

列表返回按 name 排序的 `entries`（name、description），目标入口返回 `entry`（name、description、path、content），引用返回 `reference`（path、content）。F 是规范化 PACK 根相对 `/` 路径，不含 `..`、反斜杠、尾随点/空格或最终目标逃逸；SKILL.md 用 `--entry`。只读 inspection 不创建 Session、不导入 PACK Python、不采纳知识，CLI 不遍历链接，只支持 UTF-8 文本。

结构成功只证明可发现、可读取。方法效果用相符的真实 AI 任务验证；需要 Session、采纳与恢复时使用已授权任务或隔离环境，区分未采纳引用和缺失输入。未执行行为验证如实说明，不用空 pytest 或虚构 Run 代替。

## 4. 程序开发前的复用检查

新增 Workflow、Provider、公共函数，或修改行为、输入、输出、依赖及数据处理步骤时，按顺序核对：

1. 完整读取[框架作者 API](references/base-api.md)，确认声明、Context、表、查询与物化的已有能力。
2. 完整读取 `<author-root>/references/api.md`，从短 SDK 公共目录筛选候选，再只读它链接的 `reference/` 模块文档，核对公开路径、签名、输入输出、错误和边界。
3. 按目标 inspection SDK Workflow、公共 Provider 和当前 PACK 的对应能力，检查当前 PACK 的 helpers、库文档与实际调用点；将需求拆成计算、转换、解析和来源访问，查找可组合的公共函数。
4. 实现前说明选中符号/能力及满足需求的原因；无匹配时记录实际检查范围、可复用部分和具体缺口，最终交付保留该结论，不另建状态文件。

**作者文档前提：** `api.md` 与候选 reference 由用户提供经审核、与 SDK 配对的资料，完整解压不表示已补齐。框架文档缺失，或上述 SDK 资料/必要 inspection 无法取得时，报告精确缺项并停止依赖该判断的实现。不回查 SDK 源码、不反射签名/docstring、不使用内部 API或其他版本文档兜底；不把检查失败解释为能力不存在。资料默认与 wheel 配对，不另读 metadata 比版本。知识任务及空骨架可按自己的路径继续。

已有能力满足时复用：Workflow 经 KAT 或 `ctx.run()` 组合；Provider/函数只按候选 reference 的公开路径调用。不复制实现、Guide 或薄转发声明，不通过 `kat.pack` 导入其他 PACK helpers。跨 PACK 官方能力缺口交给 `kat-dev-sdk`，当前任务只做已授权的最小 PACK 逻辑。

## 5. 程序实现与对象 inspection

实际程序放 `workflows/`、PACK 自有来源放 `datasources/`、其他 PACK 内代码放 `helpers/`、测试放 `tests/`。公共来源已有能力满足时直接 import 使用，无需加一层本地 Provider。Context 组合、来源物化、流式写入或完整数据/Guide 边界需要时，读取 [Python 详细合同](references/python-contracts.md) 的相关章节；开发 PACK 内函数库时读取[领域公共库开发](references/pack-helpers.md)。

```text
kat inspect workflow --pack P [--pack-dir D ...]
kat inspect workflow --pack P --workflow W [--pack-dir D ...]
kat inspect provider
kat inspect provider --provider V
kat inspect provider --pack P [--pack-dir D ...]
kat inspect provider --pack P --provider V [--pack-dir D ...]
```

Workflow 返回事实见共享合同。公共 Provider inspection 不带 `--pack`，读取当前 SDK；未安装 SDK 时列表为空、精确目标不存在。带 `--pack` 只检查该 PACK 自有声明；`--pack-dir` 必须与 `--pack` 一起用，两个范围可同名但不合并或自动回退。

Provider list 的 `result.providers` 按 name 排序，每项 name、description；detail 的 `result.provider` 有 name、description、module、qualname、非空 guide。Guide 说明来源语义，Python 构造与调用以 SDK reference 为准。新增/修改来源接入前先查公共 list，再读匹配 detail；仅改文案或无关分析逻辑不重复选型。

Workflow 用途、输入语义与执行前提写在 description/parameters，执行后 Guide 放 `knowledge/workflows/` 并在 decorator 关联，例如 `guide="workflows/memory-summary.md"`。Guide 解释指标单位、范围、零值/缺失含义和证据限制，不复制完整 Schema 或脚本顺序；没有依据不编造阈值。Provider Guide 放相应 knowledge 位置，复用公共来源不复制接入教程。

Workflow list/detail 不读取 Guide，作者核对本次获授权维护的文件与装饰器关联。已有获授权执行时，用成功 Run 或 `inspect run` 核对执行时快照；不为回读 Guide 自动扩大执行范围。Provider Guide 继续由 detail 回读。PACK 更新不替换旧 Session/Run 方法，知识换方法交给分析入口按用户明确要求处理。

Provider inspection 会导入所选范围的声明，任一导入、声明、重名或 Guide 无效则整体失败。模块须 import-safe，不在导入时连接服务、读取凭据、解析输入或启动进程；inspection 不实例化 Provider。失败不通过扫描源码伪造公开声明。

已授权程序创作需要补装缺失依赖时，可按[依赖管理](../kat/references/python-packages.md)使用同套 Python，无需逐包再询问；只读检查、测试或诊断不授权安装。

## 6. 验证与交付

按实际变更重新做对应 list/detail、文件归属与 Guide 关联检查。程序变更运行适用 PACK pytest：

```text
kat test --pack-dir D
kat test --pack-dir D --test tests/test_example.py::test_case
```

`--test` 可重复，直接使用 pytest 失败 node ID；路径必须是当前 PACK 的 `tests/` 相对路径，不含 root、Windows prefix 或 `..`。不转发任意 pytest argv。成功 `result.summary` 是测试结论；失败用 Response 及已返回的测试报告/日志定位。fixture 使用普通输入与配置，`kat_run` 通过真实运行边界执行，不能用 monkeypatch 假定改变子 Runtime。

交付状态和内容：

- **已完成：** 直接回答问题或说明变更、受影响文件、实际 inspection/适用测试证据与限制。程序任务保留复用结论；Guide 区分关联、测试、Run 快照和 Provider 回读。骨架展示脚本返回目录与用途；独立知识报告真实 knowledge inspection 和已做/未做的方法验证。
- **需要补充信息：** 只说明已确认事实和继续所需的最小缺项，不用虚构 owner、路径或占位能力完成任务。
- **执行失败或受阻：** 按共享合同说明 Diagnostic、证据与未通过验证，未修复变更或部分 discovery 不标为完成。安装成功不等于 PACK 验证通过，Guide 放错位置或漏关联也不能标为完成。

不原样转发完整 Response、Guide 或日志，不补造不存在的公开字段。实际未执行的验证如实说明；诊断失败本身不授权修复。
