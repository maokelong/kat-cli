---
status: accepted
---

# 文本 ftrace 转换由独立二进制程序承担

文本 ftrace 到 Parquet 的转换交付为工作区内独立的 `ftrace2parquet` 可执行文件。它不是 `kat` 子命令、Datasource、Dataset Import、PACK 能力或公共查询接口，也不读取或写入 KAT Data Home。该决定针对转换工具这一新产品边界，取代把文本 ftrace 纳入封闭 KAT Datasource 集合的方向；ADR-0013 和 ADR-0022 对 KAT 产品面本身继续有效。

`ftrace2parquet` 只接受一个 UTF-8 文本 ftrace 输入和一个 Parquet 输出目录。由该 crate 拥有的 Proto 定义唯一事件根、公共头和首批事件 oneof；每条已支持事件生成来源实例、Proto 根与所选载荷关系。合法但未注册的事件不产生表行，并在来源序号中留下间隙。转换器不建立 KAT Dataset marker、catalog、Operation log 或查询层。注释和空行被忽略；已注册事件或公共头无法完整解析时整次转换失败。

所有关系在同一临时目录完成 Parquet close 后再整体发布到尚不存在的目标目录。第一版不覆盖已有输出，不支持 stdin/stdout、压缩输入、追加、Schema 配置或容错模式。内存使用由每张关系的固定批次行数限制；输入按行读取，不把完整 trace 留在内存。
