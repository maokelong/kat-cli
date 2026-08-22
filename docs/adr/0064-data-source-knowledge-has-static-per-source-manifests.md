---
status: superseded by ADR-0069
---

# Data Source Knowledge 使用逐数据源静态清单

> 历史决定，请勿按 PACK-local 身份与目录实现；当前共享部署边界见 ADR-0069。

一个 PACK 可以在固定的数据源知识根目录下版本化多份逻辑 Data Source Knowledge；只有直接包含 `source.toml` 的一级目录才形成数据源候选，并且候选必须同时提供固定入口 `knowledge.md`。每份清单恰好包含非空根级字符串 `name`、`title`、`description` 和 `dialect`，未知字段或 TOML table 直接失败；它不承载 owner、连接位置、凭据、版本、查询列表、依赖或扩展字段。`name` 是 PACK 内唯一、稳定的小写 kebab-case 身份，完整身份是 `(PACK name, Data Source Manifest name)`；目录名只表示源码位置，移动目录不改变身份。`dialect` 只声明 SQL 语法与语义，不选择数据库驱动、不触发动态插件或安装依赖；查询执行能力必须显式声明自己支持的方言。KAT 通过这些不执行 Python 的清单生成有界目录供 AI 选择；选定数据源后先加载 `knowledge.md`，再按当前问题读取所需的 schema 文档或查询资产，而不递归拼接整个目录中的 Markdown。KAT 不扫描任意文档猜测数据源，也不把数据源列表加入当前封闭的 `pack.toml`。这保留 PACK identity 与数据源知识发现的独立演进边界，同时为开发期和分析期提供同一份确定入口。

Data Source Manifest 只形成发现与运行时解析边界，不形成 Workflow 权限或依赖声明。KAT 不增加 `required_sources`、数据源 grant 或 allowlist；业务 Workflow 可以在代码中固定数据源名称，通用 Workflow 可以通过普通字符串参数选择数据源，PACK 查询能力在实际调用时解析清单并报告未知名称或不支持的方言。Inspection 分别呈现 Workflow Interface 与数据源目录，不推断两者之间的静态关系。
