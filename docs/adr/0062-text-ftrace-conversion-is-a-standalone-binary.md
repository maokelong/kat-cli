---
status: accepted
---

# 文本 ftrace 转换由独立二进制程序承担

文本 ftrace 到 Parquet 的转换交付为工作区内独立的 `ftrace2parquet` 可执行文件。它不是 `kat` 子命令、Datasource、Dataset Import、PACK 能力或公共查询接口，也不读取或写入 KAT Data Home。该决定针对转换工具这一新产品边界，取代把文本 ftrace 纳入封闭 KAT Datasource 集合的方向；ADR-0013 和 ADR-0022 对 KAT 产品面本身继续有效。

`ftrace2parquet` 只接受一个 UTF-8 文本 ftrace 输入和一个 Parquet 输出文件。每个语法合法的事件行产生一行；公共事件头被结构化，事件名和事件分隔符后的 payload 原样保留。转换器不解释具体事件 payload，不生成按事件类型拆分的表，不建立 Proto、Dataset marker、catalog、Operation log 或查询层。注释和空行被忽略；任何非空、非注释且无法完整解析的行使整次转换失败。

输出文件由临时文件完成写入和 Parquet close 后再发布到尚不存在的目标路径。第一版不覆盖已有输出，不支持 stdin/stdout、压缩输入、目录输出、追加、Schema 配置或容错模式。内存使用由固定批次行数限制；输入按行读取，不把完整 trace 留在内存。
