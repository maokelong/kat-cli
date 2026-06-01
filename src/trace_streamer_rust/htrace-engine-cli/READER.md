# htrace-engine-cli

`htrace-engine-cli` 是 Rust TraceStreamer 的命令行入口和本地验证工具集合。

## 业务职责

- `htrace-engine inspect`: 解析 trace 文件并输出 trace 元信息、时间范围、clock domain 和各表行数。
- `htrace-engine query`: 解析 trace 文件后执行 SQL，支持直接传入 SQL 或从 SQL 文件读取，并按 `max_inline_rows` 控制内联返回行数。
- `compare-cpp-sqlite`: 将 Rust 解析结果与 C++ TraceStreamer SQLite 导出进行对比，生成 HTML/JSON 形式的验证报告。
- `sqlite_probe`: 辅助查看 SQLite 表结构和样例行，用于定位 C++ 导出数据形态。

## 对比报告职责

- 组织默认验证场景和用户指定的 trace/SQLite 输入。
- 对目标表进行行数、缺失状态和聚合 SQL 对比。
- 输出 `compare_validation_report.html`，用于观察 Rust 与 C++ 在表覆盖和关键字段聚合上的差异。

## 设计边界

- CLI 可以组织解析、查询、验证和报告，但不承载具体 trace 解析语义。
- 表结构由 `htrace-model` 维护，SQL 执行由 `htrace-query` 维护，格式解析由 `htrace-parser-harmony` 维护。
- 对比报告展示差异和覆盖情况，不替代单元测试或端到端解析验证。
