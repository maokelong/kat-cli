---
status: accepted
---

# 公共 Datasource Provider 的实现与来源知识由平台共同拥有

多个 PACK 复用文本 Ftrace 时，只有公共实现仍会让来源发现依赖某个 PACK，并迫使消费方维护重复的声明与来源 guide。KAT 在 `kat.dataprovider` 提供公共 FtraceProvider，并由平台共同维护其 `@kat.provider(name=..., description=..., guide=...)` 声明与公共来源 guide；作者无需先选中 PACK 即可发现其声明、导入位置与来源知识，消费 PACK 无需创建 `datasources/` 薄声明或复制 guide。

公共 Provider 与 PACK 自有 Provider 继续使用同一个仅附加元数据的 `@kat.provider` 装饰器。Workflow 显式 import、构造和调用公共 FtraceProvider，继续提供来源路径、clock domain 和 `ctx.datasource_root`；平台拥有公共实现不意味着运行时自动选择来源、构造 Provider 或接管其生命周期。PACK 仍可维护自己的 Provider 与 guide，具体分析问题的策略继续属于 Workflow guide。

Provider inspection 通过是否指定 `--pack` 选择唯一查询范围：不带 `--pack` 时查询平台公共 Provider，带 `--pack` 时只查询该 PACK 自有声明。两个范围不合并、不相互覆盖，也不在名称未找到时自动切换；公共 Provider 与 PACK 自有 Provider 可以同名，同一范围内的声明名称仍须唯一。公共 inspection 不依赖 PACK 发现或导入，PACK inspection 不因 Workflow 使用公共 Provider 就额外列出它。

| 命令 | 查询范围与结果 |
| --- | --- |
| `kat inspect provider` | 公共 Provider 的名称与描述列表 |
| `kat inspect provider --provider ftrace-text` | 公共 FtraceProvider 的声明、导入位置与 guide |
| `kat inspect provider --pack mem-pack` | mem-pack 自有 Provider 的名称与描述列表 |
| `kat inspect provider --pack mem-pack --provider <name>` | mem-pack 自有目标 Provider 的声明、导入位置与 guide |

首个交付切片只将文本 `FtraceProvider` 纳入公共 Provider inspection。已有 `DataFusionProvider` 继续作为本地查询与融合工具通过公共 API 和作者文档使用；Hitrace、Trace Streamer 及其他来源不在本次上移范围。该切片沿用文本 Ftrace 的现有解码、查询与物化合同，集中交付公共实现、声明、guide、无需选择 PACK 的发现入口，以及消费 PACK 的直接使用链路。

迁移时删除 mem-pack 的 FtraceProvider 薄声明和重复来源 guide，由 Workflow 直接 import 公共类，并保持分析行为与 Output 不变。mem-pack 尚无其他自有 Provider 时，其 Provider 列表为空是正常结果；旧的 `kat inspect provider --pack mem-pack --provider ftrace-text` 返回未找到，不保留兼容声明、别名或自动转向。公共来源知识由平台维护一份，领域需要额外来源行为时仍可按现有作者接口声明自己的 Provider。

本决定局部修订 ADR-0063、ADR-0071 和 ADR-0075 中所有来源 Provider 必须由 PACK 自有实现的范围，以及 ADR-0074 中 Provider declaration 与 guide 只能通过选中 PACK 发现的限制。[Issue #269](https://github.com/maokelong/kat-cli/issues/269) 与 [PR #270](https://github.com/maokelong/kat-cli/pull/270) 保留 PACK 薄声明的方案需要据此调整：公共实现与公共来源知识一起上移，PACK 自有扩展与显式生产调用的边界继续有效。

首个交付切片与验收条件见 [Issue #269 轻量 SDD](../specs/issue-269-public-ftrace-provider.md)。
