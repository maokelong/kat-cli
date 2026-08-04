# KAT 命令速查

用户只需用自然语言说明目标。以下命令由 Agent 使用；始终以 `SKILL.md` 所选平台 Payload 的绝对 `kat` 路径替代示例中的 `kat`。

除 `--help` 外，每次命令调用的 stdout 都是一个 KAT Response JSON。只在 `status="success"` 时读取 `result`；失败时读取 `error`，以及存在时的 `log_path` 或 `test_report_path`。不要从终端文本、日志或 pytest 输出推断成功。

## 查看帮助

```text
kat --help
kat <命令> --help
```

用于确认固定 CLI 路由和外层参数。Workflow 的业务参数不在 `kat run --help` 中；必须先执行 `kat inspect --pack <名称>`，再按 Response 的 `parameters` 在 `--` 后传入。

## 导入 Trace Streamer SQLite（Deprecated 预发布）

```text
kat import trace-streamer --database <本地SQLite路径>
kat import --dataset <Dataset目录> trace-streamer --database <本地SQLite路径>
```

只在用户明确要求试用或验证时，用于把一个本地 Trace Streamer SQLite 数据库转为受管理 Dataset。调用前必须说明该入口及依赖它的 PACK 均为 Deprecated 预发布能力，不承诺稳定 Schema、生产兼容性或迁移路径。第二种形式指定结果目录；只有用户明确要求替换该目录时，才可额外传入 `--overwrite-dataset`，因为它会永久删除目录中原有的全部内容。成功后只从 `result.path` 取得规范化 Dataset 路径，随后用该路径执行 `kat inspect --dataset`。

`kat import hitrace --trace <本地.htrace路径>` 虽可用于明确的导入检查，但当前没有完成长期 `.htrace` Bundled Workflow 闭环。不要把导入成功表述为分析完成，也不要把 `.htrace` 请求改写为 Trace Streamer 请求。

## 检查 Dataset 或发现 PACK

```text
kat inspect --dataset <Dataset目录>
kat inspect
kat inspect --pack <PACK名称> [--pack-dir <PACK目录> ...]
```

- 第一条只读检查 Dataset；成功 `result` 中的 `path`、`tables` 与 schema 是后续匹配的事实来源。
- 第二条发现当前可用的 PACK；成功 `result.packs` 为空也是有效结果。
- 第三条检查一个精确 PACK；`--pack-dir` 可重复，每个目录必须直接包含 `pack.toml`。成功 Response 的 Workflow、`required_tables` 与 `parameters` 决定能否运行以及如何传参。

## 执行 Workflow

```text
kat run --pack <PACK名称> --workflow <Workflow名称> \
  [--dataset <Dataset目录>] [--pack-dir <PACK目录> ...] -- \
  <来自 inspection.parameters 的 Workflow 参数>
```

用于执行已检查且 Dataset tables 满足 `required_tables` 的 Workflow。`--` 后的 token 原样交给 Workflow Input Compiler；只传 inspection 明示的 option，且不要包含秘密，因为这些参数可能写入 Operation log。成功后从 `result.run_id` 和 `result.outputs` 取得唯一可查询 Run 及其输出元数据。

## 查询既有 Run

```text
kat query --run <Run ID> --sql <一条 SQL>
```

用于只读查询已发布的 Run。SQL 只访问本次问题需要的列和行，并在 SQL 中自行加入投影、过滤、聚合与排序；明细查询在发送前显式写入 `LIMIT`。成功 `result.columns` 与 `result.rows` 是分析证据；不要读取 Run 文件、假设 KAT 会自动添加限制，或把实际执行失败包装成部分成功。

构造 SQL 前必须已有 Run Output 名称与 columns：优先使用刚完成的 `kat run` 成功 Response 中的 `result.outputs`，也可以使用用户提供的同等元数据。只有 Run ID 时没有受支持的 Run Output 发现命令；应请求用户补充元数据并停止，不猜表名、不读取 Run 内部文件。

## 测试一个 PACK

```text
kat test --pack-dir <PACK目录>
kat test --pack-dir <PACK目录> --test <pytest node ID> [--test <pytest node ID> ...]
```

用于在生产执行平面运行该 PACK 的 pytest。`--pack-dir` 是一个直接包含 `pack.toml` 的精确目录，不使用 PACK 名称。成功 `result.summary` 是测试结论；失败时引用 Response 的 Diagnostic，以及存在时的 `test_report_path` 和 `log_path`。测试或诊断失败不授权修改 PACK。
