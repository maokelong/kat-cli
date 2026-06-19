# kat-rs 交互面收敛设计

## 背景

`kat-rs` 仍处于早期阶段，没有需要兼容的历史用户契约。当前已经同时存在两类功能入口：

1. CLI：`kat-rs query --source ... --sql ...`。
2. 本机 REST API：`POST /v1/datasources` 与 `POST /v1/datasources/{datasourceId}/queries`。

二者暴露同一类能力，却需要分别维护参数语义、错误呈现、文档示例和测试覆盖。随着 #45 规划中的 dataset、pack、derived table、analysis run、state、evidence 等资源继续出现，如果继续让 CLI 和 REST 并行扩张，每个功能都要重复设计两套交互契约。

本设计选择在早期阶段做破坏性收敛：REST/OpenAPI 成为唯一业务功能 API，CLI 只保留本机 runtime 生命周期管理。

## 要解决的问题

1. 消除 CLI query 与 REST query 的双界面维护成本。
2. 为后续 dataset、pack、derived table、analysis run 和 evidence 建立单一资源模型。
3. 让 OpenAPI 成为功能接口描述的事实来源，减少 README 与 CLI help 的漂移。
4. 保持当前 datasource/query 内核不变，只收敛交互面。

## 不做什么

1. 不实现新的 dataset、pack、derive 或 analyze 资源；这些继续由 #45、#59、#60 及后续 issue 拆分。
2. 不在本切片重命名 workspace crate，例如 `kat-rs-daemon` 可以继续作为内部 crate 名存在。
3. 不保留 `kat-rs query` 的 deprecated 兼容期。
4. 不新增 CLI 作为 REST client 的业务命令包装层。
5. 不引入远程访问、鉴权、TLS、多用户隔离或自动拉起。

## 方案选择

采用破坏性收敛方案：

```text
用户 / MCP / UI / curl / 生成客户端
  -> REST API
  -> kat-rs server runtime
  -> datasource / dataset / pack / analysis 内核
```

CLI 只保留 runtime 管理：

```text
kat-rs serve
kat-rs stop
kat-rs openapi
kat-rs version
```

备选方案及取舍：

1. 保留 CLI query 并标记 deprecated，迁移更温和，但项目没有历史债务，保留过渡层只会继续消耗文档和测试成本。
2. 让 CLI query 包装 REST API，可以复用服务端实现，但仍然形成第二套用户契约，未来 `ingest/derive/analyze/pack` 也会反复面临同样问题。
3. CLI 与 REST 长期并行，短期最省改动，但与 #45 的资源化方向冲突最大。

## 交互面边界

REST/OpenAPI 是唯一业务功能面。trace/log 查询、dataset 操作、pack 校验、derived table 物化、analysis run、state 和 evidence 都应表现为 HTTP 资源或资源动作。

CLI 不解释业务功能参数，不承诺业务功能输出格式。CLI help 只解释：

1. 如何启动本机 server。
2. 如何关闭本机 server。
3. 如何输出 OpenAPI。
4. 默认监听地址、loopback 限制、日志环境变量和版本信息。

现有 `kat-rs query --source ... --sql ...` 直接删除。用户需要先启动 server，再通过 REST API 创建 datasource 并查询：

```text
kat-rs serve --host 127.0.0.1 --port 3030
POST /v1/datasources
POST /v1/datasources/{datasourceId}/queries
```

## 用户可见命名

`daemon` 是实现视角，用户可见命名应收敛为 `serve` / `server`。第一刀只调整 CLI 命令和 README 表述，不强制重命名内部 crate。

建议命令：

```text
kat-rs serve --host 127.0.0.1 --port 3030
kat-rs stop --host 127.0.0.1 --port 3030
kat-rs openapi
kat-rs version
```

`serve` 前台运行，并继续强制只监听 loopback IP。`stop` 仍可调用 `DELETE /v1/server`。`openapi` 输出与 server 暴露的 OpenAPI 同源的 JSON。

不保留 `kat-rs daemon start` / `kat-rs daemon stop` 兼容别名。

## REST 与 OpenAPI

第一刀保留现有 REST 资源行为：

```text
GET    /v1/health
POST   /v1/datasources
GET    /v1/datasources?limit=100&offset=0
GET    /v1/datasources/{datasourceId}
DELETE /v1/datasources/{datasourceId}
POST   /v1/datasources/{datasourceId}/queries
DELETE /v1/server
```

新增 OpenAPI 暴露面：

```text
GET /openapi.json
```

OpenAPI 描述现有 request、response、pagination、meta 和 error envelope。README 只保留少量 curl 示例，并指向 OpenAPI 作为完整接口描述，不再维护 CLI query 参数矩阵。

## 实现边界

`kat-rs-cli`：

1. 删除 `Command::Query`、`QueryArgs`、`SourceArg` 和 `run_query`。
2. 将 `daemon start/stop` 改为顶层 `serve` / `stop`。
3. 新增 `openapi` 命令，输出同一份 OpenAPI JSON。
4. `kat-rs-cli` 对 `kat-rs-datasource` 的 Cargo 依赖可后续单独 cleanup，不作为本切片交付要求。

`kat-rs-daemon`：

1. 保留 datasource registry、query service 和现有 REST envelope。
2. 新增 `/openapi.json` 路由。
3. 提供可由 CLI `openapi` 复用的 OpenAPI 生成函数。

`kat-rs-datasource`：

1. 不做交互面相关改动。
2. 继续只承担输入适配、Arrow 物化和 DataFusion 查询。

## 错误模型

REST 继续使用现有统一错误 envelope。OpenAPI 必须描述错误响应结构。

CLI 只处理 runtime 生命周期错误，例如：

1. 非 loopback host。
2. server 绑定失败。
3. `stop` 无法连接或未收到预期 shutdown 响应。
4. `openapi` 输出失败。

CLI 不再承担 SQL、datasource 创建、schema 推断、查询失败等业务错误呈现。

## 测试与验证

本切片的测试重心从 CLI query 转移到 REST API contract：

1. 删除 CLI query e2e 和 query help 断言。
2. 保留并更新 server API contract：创建 datasource、复用 identity、查询、分页、删除 datasource、关闭 server、错误 envelope。
3. 新增 OpenAPI contract：`GET /openapi.json` 返回合法 JSON，包含现有资源路径。
4. 新增 CLI contract：help 只列出 `serve`、`stop`、`openapi`、`version`；`serve/stop` 继续拒绝非 loopback host；`openapi` 输出与 server OpenAPI 同源。
5. README 删除“命令行查询”主路径，改为 REST 使用流程。

提交前验证：

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python .github/scripts/test_pr_guard.py
git diff --check
```

## 最小交付切片

第一刀只交付交互面收敛：

1. 删除 `kat-rs query`。
2. 将用户可见启动命令改为 `kat-rs serve`。
3. 将用户可见停止命令改为 `kat-rs stop`。
4. 增加 OpenAPI 输出和 `/openapi.json`。
5. 更新 README 和测试。

不把 #59/#60 的本地 dataset 持久化接入混入本切片。后续 dataset、pack 和 analysis runtime 都应直接接入 REST/OpenAPI 资源模型，而不是先增加 CLI 业务命令。
