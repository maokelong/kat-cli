---
status: accepted
---

# 文本 ftrace 原生解码能力作为 PACK Provider 的解析后端

文本 ftrace 到类型化 Parquet 的转换由 `kat-datasource` Rust 内核承担，并通过现有原生 wheel 的 `kat_datasource.text_ftrace.decode()` 交付给 Python Provider。解码函数只接受来源、目标目录和明确的 Clock domain，拥有文本语法、Proto 类型合同、有限批次写入与目录原子发布；它不是独立可执行文件或 `kat` 子命令，也不读取或写入 KAT Data Home。选择项目自有解析器是因为当前合同要求验证事件结构列头、忽略无助于分析且可能只有格式占位符的 buffer 展示统计、保留未知事件产生的来源序号间隙、接受调用方 Clock domain，并发布 KAT 自有 Proto 派生关系；Perfetto Trace Processor 面向其标准 SQL 表和宽松导入语义，不能直接替代这些来源合同。

PACK 用普通 Python `FtraceProvider` 调用该原生 API。Provider 把目标固定为 Datasource root 与来源文件名组成的目录。目录已经包含 Parquet 时只打开并执行关系与 `clock_domain` 准入检查，不再次解析；目录不存在或为空时才解析。非空但没有 Parquet、无法打开、缺少必需关系或 domain 不匹配都明确失败，不隐式删除或重建。合法但未支持的事件名称以排序去重的 decode report 和可复用关系交付；只包含未支持事件的合法来源仍能查询 header 和报告。

同一 Datasource root 下的同名来源共享目标目录；需要区分同名内容时由调用方选择不同文件名或 Datasource root。Provider 不提供重新解析、自动清理或显式结束接口。调用方只构造 Provider 并使用 `query() -> dp.Table`。

该决定以 ADR-0063、ADR-0067 和 ADR-0069 确定的 PACK 所有权、Catalog 与物化边界接入当前 Datasource Toolkit，并承接 ADR-0070 的原生 Python 调用边界。原生解码函数不成为 Pack Authoring API 或公共查询接口；显式 Schema 版本、迁移、自动回收和并发协调不在本切片承担。
