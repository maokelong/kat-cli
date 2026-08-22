---
status: accepted
---

# PostgreSQL SQL 文件使用显式 UTF-8 路径输入

`execute_sql_file()` 的 `sql_file_path` 接受 `str` 或 `os.PathLike[str]`，拒绝 bytes 路径，并继续要求最终路径为绝对路径。common 使用严格的 `utf-8-sig` 在每次调用时重新读取完整文件，因此同时接受无 BOM 与带 UTF-8 BOM 的 SQL，但不缓存文件内容，也不接受无效编码。

common 不自动展开 `%ENV%`、`~`、通配符或其他路径表达式；Workflow 负责在调用前从常量、`__file__`、Workflow 输入或环境配置构造最终绝对路径。该合同避免依赖 Host 当前目录或隐藏的路径解析规则，同时保留 Windows PACK 使用 `pathlib.Path` 的自然写法。
