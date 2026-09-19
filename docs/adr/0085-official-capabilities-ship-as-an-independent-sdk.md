---
status: accepted
---

# 官方公共能力通过独立 SDK 交付

各领域需要复用同一套具体 Provider、Workflow 和公共函数，并在不替换 CLI 与 Runtime 的情况下升级能力。将这些实现留在私有 Host wheel 中，会把领域能力更新绑到整套 KAT 发布；将框架整体搬入 SDK 又会扩大兼容面。本决定采用独立的官方公共能力 distribution `kat-sdk`，Python namespace 为 `kat_sdk`；框架 API、Runtime、Context、装饰器和标准表工具继续由 `kat-workflow` 提供，原生来源解码继续由 `kat-datasource` 提供。

SDK 根直接拥有一个公共 PACK，源码为 `kat/sdk/pack.toml`，Workflow 位于 `workflows/`，不增加 packs 层。当前 KAT 的 Bundled Python 定位已安装 SDK 根，CLI 将该精确目录加入正常发现集合；同一目录去重、不同目录同名失败。顶层执行、组合执行与 PACK 测试沿用同一发现范围和 Runtime 合同。SDK 缺失或损坏是安装问题，不返回看似完整的部分能力列表，也不为公共 PACK 设置覆盖优先级。

公共 Provider 的模块清单由 SDK 拥有，Runtime 读取固定 SDK 入口，不硬编码具体 Provider。公共 inspection 与 PACK 自有范围继续隔离；读取声明和知识时不实例化来源。Provider、Workflow 的实现与 Guide 随 SDK 一起版本化。普通公共函数是直接 import 的 Python API，不建立 KAT 函数发现命令；AI 通过当前 KAT Python 定位 `knowledge/index.md` 并读取相对链接中的方法说明。

SDK 的手写导航和 Guide 与构建期生成的 API Markdown 分开维护。类型注解与 docstring 是 API 参考权威来源，构建使用 Griffe/Griffe2MD 静态解析，不执行业务模块，也不自行实现 Python 文档解析器。Framework 的已有原子目录发布能力以窄的 `publish_materialization` Toolkit 方法公开，TraceStreamerProvider 无需继续依赖框架私有符号或复制通用实现。

首个切片迁移 FtraceProvider、TraceStreamerProvider 及其知识、消费者和行为测试，不保留旧 import 别名。生产 SDK 暂无正式公共 Workflow 或公共函数；测试专用能力只验证发现、调用、文档与升级链路。公共算法仍需经过真实消费者验证后才能晋升，不把领域示例直接当作正式 SDK 能力。

SDK 从 0.1.0 独立版本化，最低新框架基线为 0.1.1rc13；已发布 rc12 不具备该发现机制。首版沿用 CPython 3.14 与 Windows/Linux x86_64，发布 wheel 与 SHA256，经当前 KAT Python 的 pip 安装升级。元数据依赖范围只限制可安装候选，正式发布仍须由双平台安装、查询、组合执行和 SDK 升级验证提供证据；正常分析不主动联网更新。

本决定局部替代 ADR-0045 中具体公共能力只能随私有 Host 原子交付、ADR-0049 中公共 Trace 库发布位置与不建立公共 PACK、ADR-0081 中公共 Provider 位于框架的归属，并扩充 ADR-0007 的发现来源。框架所有权、真实消费者准入、Provider 查询范围隔离及 ADR-0017 的入口规则继续有效。完整切片与验收见 [SDK SDD](../specs/public-capability-sdk.md) 和 [Issue #296](https://github.com/maokelong/kat-cli/issues/296)。
