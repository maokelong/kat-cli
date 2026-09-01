---
status: accepted
---

# 文本 ftrace 转换通过 Datasource Python 包交付

文本 ftrace 到 Parquet 的转换作为 `kat-datasource` Rust 内核的一项来源解码能力，通过现有平台原生 wheel 中的 `kat_datasource.text_ftrace.decode()` 交付给 Python Datasource Provider。它不是独立可执行文件、`kat` 子命令、平台注册的 Datasource type、Dataset Import、PACK 能力或查询接口，也不读取或写入 KAT Data Home。Python Provider 仍拥有来源选择、私有物化位置、复用、清理、Catalog 准入和查询；原生扩展只同步执行一次显式输入到显式目标的转换。

转换函数只接受一个 UTF-8 文本 ftrace 输入、一个 Parquet 输出目录和显式 clock domain。由 `kat-datasource` 拥有的 Proto 定义唯一事件根、公共头和首批事件 oneof；每条已支持事件生成来源实例、Proto 根与所选载荷关系。合法但未注册的事件不产生表行，并在来源序号中留下间隙。转换器不建立 KAT Dataset marker、Operation log 或查询层。注释和空行被忽略；已注册事件或公共头无法完整解析时整次转换失败。

所有关系在同一临时目录完成 Parquet close 和 footer 校验后再整体发布到尚不存在的目标目录。PyO3 包装在转换期间释放 Python 解释器，并把解码失败映射为 `kat_datasource.text_ftrace.DecodeError`。第一版不覆盖已有输出，不支持压缩输入、追加、Schema 配置或容错模式。内存使用由每张关系的固定批次行数限制；输入按行读取，不把完整 trace 留在内存。
