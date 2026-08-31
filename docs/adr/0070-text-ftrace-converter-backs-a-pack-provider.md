---
status: accepted
---

# 文本 ftrace 转换器作为 PACK Provider 的解析后端

文本 ftrace 到类型化 Parquet 的转换继续由工作区内独立的 `ftrace2parquet` 可执行文件承担。转换器只接受来源、目标目录和明确的 Clock domain，拥有文本语法、Proto 类型合同、有限批次写入与目录原子发布；它不是 `kat` 子命令，也不读取或写入 KAT Data Home。

PACK 可以用普通 Python `FtraceProvider` 封装该二进制。Provider 从部署环境解析可执行文件，在当前 Workflow 授予的 Datasource storage root 下管理私有临时 workspace，并负责进程调用、必需关系校验、失败清理和 `query() -> dp.Table`。它不重新解析文本、不复制关系 Schema，也不注册到 KAT。转换器、Catalog 路径和 Parquet 物理格式不进入 Provider 使用者合同；最终 eager Table 由 Runtime 发布为 Run Output。

该决定保留转换器独立、可测试的产品边界，同时以 ADR-0063、ADR-0067 和 ADR-0069 确定的 PACK 所有权、Catalog 与 Workflow 生命周期接入当前 Datasource Toolkit。它取代同名旧 ADR 中“不是 PACK 能力”的绝对限制；二进制本身仍不成为 Pack Authoring API 或公共查询接口。
