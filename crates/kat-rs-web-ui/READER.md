# kat-rs-web-ui

`kat-rs-web-ui` 是 kat-rs 的本地 Web 查询界面。它通过
`kat-rs-datasource` 打开 trace 数据集，提供 inspect 和 SQL query 能力，
用于人工浏览、调试和验证解析结果。

## 业务职责

- 启动本地 HTTP 服务，并为启动参数传入的 trace 文件建立当前 datasource session。
- 列出 `tests/fixtures/traces` 下的 fixture trace，并允许在页面中打开、解析和切换查看。
- 接收用户上传的 trace 文件，保存到本地临时目录后通过 datasource 打开。
- 维护本进程内的 dataset registry，让当前数据、fixture 数据和上传数据可以独立选择。
- 提供 `/api/inspect`，返回 schema、表清单、列信息、行数和 trace 基本信息。
- 提供 `/api/query`，接收 SQL 并返回 `kat-rs-datasource` 的 JSON 查询结果。
- 提供单页前端界面，用于浏览表、查看统计、编写 SQL、检查查询结果和调试解析输出。

## HTTP 接口

- `GET /api/datasets`: 返回已打开 dataset、当前 active dataset id 和可打开 fixtures。
- `POST /api/datasets/fixture`: 传入 fixture 相对路径，打开并切换到该 dataset。
- `POST /api/datasets/upload`: multipart 上传字段名为 `trace`，保存后打开并切换到该 dataset。
- `GET /api/inspect?dataset_id=...`: 返回指定 dataset 的 inspect 数据；不传时使用 active dataset。
- `POST /api/query`: 请求体可带 `dataset_id`、`sql`、`max_inline_rows`；不传 dataset id 时使用 active dataset。

## 设计边界

- Web UI 不直接解析 trace 文件，也不复制 parser/model/query 的业务逻辑。
- 新表展示应优先通过 inspect/query 接口读取，避免在前端硬编码解析规则。
- 本模块面向本地调试和人工检查，不负责持久化 trace 数据。
- 上传目录仅用于本地调试会话，不作为产品级 artifact/cache 管理。
- CLI、Skill、MCP 和未来服务化入口都应通过 datasource contract 接入，而不是绕过 datasource 直接访问 parser。
