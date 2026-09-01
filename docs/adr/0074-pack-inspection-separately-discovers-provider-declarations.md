---
status: accepted
---

# Workflow 与 Provider 使用独立 inspection 模式

无目标 `kat inspect` 继续只从静态 manifest 返回 PACK 索引，不启动 Python。选中
PACK 后，分析问题使用 `kat inspect workflow --pack <pack-name>`，PACK 作者发现
来源能力时使用 `kat inspect provider --pack <pack-name>`；两种模式分别扫描、校验
并返回自己的 declaration，不把 Provider 混入 Workflow 结果。只有 Run ID 时，
`kat inspect workflow --run <run-id>` 从 Run Manifest 定位当前 PACK 和 Workflow，
再读取当前安装版本的 declaration 与 guide。该定位只读取 Run identity，不校验或
解释 Manifest 中的 Dataset、inputs 与 outputs 数据合同。

Workflow 与 Provider 列表都只包含显式声明的 `name` 和 `description`。选中
Workflow 后才返回 `parameters` 和可选 `guide`；选中 Provider 后才返回声明 class
可静态 import 的 `module`、`qualname` 和必填 `guide`。Guide 是 declaration 引用的
PACK-owned 不透明 Markdown：路径必须相对 `knowledge/`，Provider 与 Workflow 分别
位于 `providers/` 和 `workflows/` 子目录；目标必须是根内已存在、非空、有效 UTF-8
的普通 `.md` 文件，Response 只返回原始正文而不暴露物理路径。

`@kat.workflow` 的公共身份改为必填、非空的 `name` 与 `description`，不再声明
`title`、从 docstring 推导 description 或声明 `required_tables`。`@kat.provider`
是只接受 class 的 metadata-only decorator，只声明 `name`、`description` 和
`guide`；它不要求基类、固定方法、factory 或生命周期，也不改变普通 Python 类的
构造和调用方式。

Provider inspection 递归导入所选 PACK `datasources/` 树中的普通 Python module，
只收集各 module 自己定义且可由公开 `module + qualname` 静态解析回原 class 的
Provider declaration。它不实例化、连接或调用 Provider。任一 import、declaration、
名称唯一性或 guide 校验失败都会使本次 inspection 原子失败。生产 Workflow Runtime
仍不扫描、注册、构造或包装 Provider；Workflow 继续显式 import、构造和调用普通
Provider 类。

本决定取代 ADR-0014 与 ADR-0016 中 `kat inspect --pack`、`kat inspect --dataset`
和 Required tables inspection 的公共合同，也取代 ADR-0015 的 Workflow
`required_tables` declaration 与 Table Grant 约束。它修订 ADR-0004 的 Runtime
operation/启动边界、ADR-0007 的 PACK discovery 命令集合，以及 ADR-0063 中
“Runtime 从不扫描 Provider”为“只有显式 Provider inspection 扫描 metadata
declaration”。这些替代只建立 Agent Knowledge 与 inspection 产品面；为使本改动
不依赖其他数据架构 Issue，现有 Dataset 执行参数暂时保留；移除
`required_tables` 后，提供 Dataset 时其全部 Source table 对 Workflow 可见。
Output Query 沿用当前主线合同，只查询 `output.*` 并发布原生 NDJSON，不读取历史
Dataset。除此之外，本决定不修改 Dataset、Query 的生命周期或数据架构。
