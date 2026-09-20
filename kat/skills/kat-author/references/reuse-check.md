# 开发前复用检查

SDK 与领域 PACK 新增或扩展 Workflow、Provider、公共函数前均执行以下检查；仅修改文档或不改变能力的变更无需重复选型。领域开发检查正在开发的 PACK 和官方 SDK；SDK 开发检查 SDK 源码及本次需求明确涉及的领域 PACK。

1. 先检查是否已有同类实现。Workflow 从 [SDK Workflow 导航](../../kat/references/workflows/index.md) 和正在开发的 PACK 的 Workflow list/detail 核对用途、参数及 Guide；Provider 从 [SDK Provider 导航](../../kat/references/providers/index.md)、公共及正在开发的 PACK 的 Provider list/detail 核对来源合同；公共函数查 [SDK helpers 导航](../../kat/references/helpers/index.md) 及正在开发的 PACK 的 `helpers/`、`knowledge/helpers/` 和实际调用点。inspection 命令见 [命令合同](../../kat/references/command-reference.md)。SDK 源码开发还须检查 `kat/sdk/` 的对应实现，避免遗漏尚未安装的能力；领域开发检查正在开发的 PACK 的对应源码。inspection 失败不能用源码扫描伪造成功的公开声明。
2. 开发 Workflow 或 Provider 时，进一步按所需计算、转换、解析等步骤检查公共函数，而不只查同类入口。查阅 SDK helpers 导航、`kat/sdk/helpers/`（源码可用时）及正在开发的 PACK 的 helpers 和文档，核对用途、导入路径、适用版本、输入输出、语义、依赖与限制；使用已安装 SDK 时按需核对签名和 docstring。公共函数服务于具体 Workflow/Provider，新增函数要说明实际消费者与需要复用的职责。
3. 已有能力满足需求时直接复用：Workflow 通过 KAT 执行或 `ctx.run()` 组合，Provider 直接 import、构造并调用，公共函数直接 import 并调用。复用现有文档，不复制实现或增加仅转发调用的重复能力；不同 PACK 不通过 `kat.pack` 互相导入 helpers。
4. 确有缺口时，记录已查范围、候选能力、可复用部分及具体缺口，只实现已授权的必要部分。没有匹配项也须说明检查范围，不能只凭名称判定无法复用。
5. 无法取得判断所需的文档、源码或安装信息时，说明原因并停止依赖该判断的开发，不将检查失败当作能力缺席。交付时说明实际复用选择或自实现缺口，并验证函数行为及 Workflow/Provider 的真实调用链路。
