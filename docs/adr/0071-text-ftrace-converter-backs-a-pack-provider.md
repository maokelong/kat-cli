---
status: accepted
---

# 文本 ftrace 原生解码能力作为 PACK Provider 的解析后端

文本 ftrace 到类型化 Parquet 的转换由 `kat-datasource` Rust 内核承担，并通过现有原生 wheel 的 `kat_datasource.text_ftrace.decode()` 交付给 Python Provider。解码函数只接受来源、目标目录和明确的 Clock domain，拥有文本语法、Proto 类型合同、有限批次写入与目录原子发布；它不是独立可执行文件或 `kat` 子命令，也不读取或写入 KAT Data Home。选择项目自有解析器是因为当前合同要求验证事件结构列头、忽略无助于分析且可能只有格式占位符的 buffer 展示统计、保留未知事件产生的来源序号间隙、接受调用方 Clock domain，并发布 KAT 自有 Proto 派生关系；Perfetto Trace Processor 面向其标准 SQL 表和宽松导入语义，不能直接替代这些来源合同。

PACK 用普通 Python `FtraceProvider` 调用该原生 API。Provider 以来源文件内容的 SHA-256 作为内部目录名，在当前 PACK 的 Datasource root 下跨 Workflow 复用已经通过准入检查的 Parquet。相同内容命中后必须验证已保存的 `clock_domain` 与请求一致；不同 domain 明确失败，不静默改写同一内容身份。旧目录打开失败、必需关系缺失或损坏时直接丢弃并重建。合法但未支持的事件名称以排序去重的 decode report 和可复用关系交付；只包含未支持事件的合法来源仍能查询 header 和报告。

`redecode` 与 `auto_cleanup` 分别控制是否丢弃旧物化后重新解析，以及 `finish()` 时是否删除本次采用的内容目录。四种组合都有效：默认复用并保留、复用后清理、重解析并保留、重解析后清理。调用方负责协调同一内容的并发查询、重解析和清理；Provider 不提供跨 Workflow 并发保障。调用方不感知转换器、Catalog 路径或 Parquet 物理格式，只构造 Provider、使用 `query() -> dp.Table`，并在 eager Table 脱离来源后调用 `finish()`。

该决定以 ADR-0063、ADR-0067 和 ADR-0077 确定的 PACK 所有权、Catalog 与可重建物化边界接入当前 Datasource Toolkit，并承接 ADR-0070 的原生 Python 调用边界。原生解码函数不成为 Pack Authoring API 或公共查询接口；显式 Schema 版本、迁移和自动回收不在本切片承担，解码合同变化后的重新解析由调用方显式选择。
