# htrace-engine-cli

`htrace-engine-cli` 是 Rust TraceStreamer 的命令行入口。

## 业务职责

- `htrace-engine inspect`: 解析 trace 文件并输出 trace 元信息、时间范围、clock domain 和各表行数。
- `htrace-engine query`: 解析 trace 文件后执行 SQL，支持直接传入 SQL 或从 SQL 文件读取，并按 `max_inline_rows` 控制内联返回行数。
- 为本地调试、脚本化查询和人工检查提供稳定的命令行入口。

## 设计边界

- CLI 可以组织解析、查询和输出格式，但不承载具体 trace 解析语义。
- 表结构由 `htrace-model` 维护，SQL 执行由 `htrace-query` 维护，格式解析由 `htrace-parser-harmony` 维护。
