# Trace Streamer SQLite Provider

该 Provider 打开 Workflow 通过 `sqlite_path` 指定的一份 Trace Streamer SQLite 数据库，并执行只读 SQL。

- `sqlite_path` 必须是现存普通文件的精确绝对路径。
- `query()` 必须显式传入结果 `schema`，SQL 返回列名及顺序必须与其一致。
- 可通过 `params` 传入命名参数；写操作和修改数据库状态的 SQL 会被拒绝。
- 查询结果是可直接作为 Workflow Output 返回或参与融合查询的 `dp.Table`。
