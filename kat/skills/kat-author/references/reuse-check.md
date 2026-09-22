# 开发前复用检查

新增 Workflow、Provider 或公共函数，或者修改其行为、输入、输出、依赖或数据处理步骤前，均执行本检查。只改文案、格式，或只改不影响能力的测试时可以跳过。本文只约束领域 PACK 创作；SDK 开发使用 `kat-dev-sdk` 自己的源码侧流程。

按以下顺序读取，不能跳过前一层直接从实现猜能力：

1. 完整读取 [框架作者 API](base-api.md)，确认 `kat-workflow` 已提供的声明、组合、表、查询和物化能力。
2. 完整读取本目录的 `api.md`。它是短的 SDK 公共模块目录；按一句话用途筛选与当前需求相关的模块。
3. 只读取候选模块链接到的 `reference/<module>.md`，核对公开导入路径、签名、输入、输出、公开错误、行为边界与经验证示例。不能只凭模块名或符号名判定可复用性。
4. 按 [命令合同](../../kat/references/command-reference.md) 执行 Workflow 和 Provider inspection。SDK Workflow 先 list 再读取候选 detail，核对参数和 Guide；公共 Provider 先 list 再读取候选 detail，核对声明与来源 Guide。随后对正在开发的 PACK 执行相应 list/detail，并检查其 `helpers/`、`knowledge/helpers/` 和实际调用点。inspection 是 Workflow/Provider 的公开事实来源，不能被 Python API 文档替代。

开发 Workflow 或 Provider 时，除同类入口外，还要把需求拆成计算、转换、解析和来源访问等步骤，在 SDK API 文档与当前 PACK 中分别检查可组合的公共函数。公共函数必须服务于具体消费者；新增函数时说明消费者及其需要复用的职责。

已有能力满足需求时直接复用：Workflow 通过 KAT 执行或 `ctx.run()` 组合；Provider 和公共函数只按 SDK reference 给出的公开路径 import 和调用。复用现有 Guide 或 reference，不复制实现，不增加只有转发作用的重复能力；不同 PACK 不通过 `kat.pack` 互相导入 helpers。

开始实现前，在当前任务的实现说明中记录复用结论：列出选中的 `kat_sdk.<module>.<symbol>`、满足需求的原因和对应 reference；选中 Workflow 或 Provider 时同时记录 inspection 得到的名称与 Guide。没有匹配项时，记录实际读取的 `base-api.md`、`api.md`、候选 reference、inspection 范围、可复用部分及具体缺口。交付说明中保留这份结论，不另建仓库状态文件。

如果框架 API、SDK API 文档和 inspection 均没有满足需求的能力，只实现已授权的最小 PACK 内逻辑；可能成为跨 PACK 官方能力的缺口交给 `kat-dev-sdk`。不得扫描 SDK 源码、导入未进入 SDK API 文档的符号，或使用内部 API。

如果 `base-api.md`、`api.md`、判断所需的候选 reference 或 inspection 无法取得，报告具体缺项并停止依赖该判断的实现。不得用 SDK 源码、运行时签名或 docstring、其他版本文档兜底，也不得把检查失败解释为不存在公共能力。当前部署提供的文档默认已经与 wheel 匹配，不额外读取 distribution metadata 或比较版本。
