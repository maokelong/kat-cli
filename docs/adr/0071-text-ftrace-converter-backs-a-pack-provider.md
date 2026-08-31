---
status: accepted
---

# 文本 ftrace 转换器作为 PACK Provider 的解析后端

文本 ftrace 到类型化 Parquet 的转换继续由工作区内独立的 `ftrace2parquet` 可执行文件承担。转换器只接受来源、目标目录和明确的 Clock domain，拥有文本语法、Proto 类型合同、有限批次写入与目录原子发布；它不是 `kat` 子命令，也不读取或写入 KAT Data Home。选择项目自有解析器是因为当前合同要求严格验证 Hitrace header、保留未知事件产生的来源序号间隙、接受调用方 Clock domain，并发布 KAT 自有 Proto 派生关系；Perfetto Trace Processor 面向其标准 SQL 表和宽松导入语义，不能直接替代这些来源合同。

PACK 用普通 Python `FtraceProvider` 封装该二进制。Provider 从部署环境解析可执行文件，以来源文件内容的 SHA-256 作为内部目录名，在当前 PACK 的 Datasource root 下跨 Workflow 复用已经通过准入检查的 Parquet。相同内容命中后必须验证已保存的 `clock_domain` 与请求一致；不同 domain 明确失败，不静默改写同一内容身份。旧目录打开失败、必需关系缺失或损坏时直接丢弃并重建。并发转换同一内容时，转换器的原子目录发布决定唯一成功者，另一方校验并采用已发布结果。

`auto_cleanup=True` 是显式临时模式：Provider 改用实例独占的 `TemporaryDirectory`，对象回收或进程退出时删除，不读取或删除稳定 SHA-256 目录。默认模式保留内部物化结果。两种模式都不向调用方暴露转换器、Catalog 路径或 Parquet 物理格式；调用方只构造 Provider 并使用 `query() -> dp.Table`，最终 eager Table 由 Runtime 发布为 Run Output。

该决定以 ADR-0063、ADR-0067 和 ADR-0069 确定的 PACK 所有权、Catalog 与可重建物化边界接入当前 Datasource Toolkit，并承接 ADR-0070 的独立转换器产品边界。二进制本身仍不成为 Pack Authoring API 或公共查询接口；显式 Schema 版本、迁移和物化回收不在本切片承担。
