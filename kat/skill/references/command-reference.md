# KAT 命令速查

用户只需用自然语言说明目标。以下命令由 Agent 使用；始终以 `SKILL.md` 所选 Platform Payload 的绝对 `kat` 路径替代示例中的 `kat`。

除 `--help` 外，每次命令调用的 stdout 都是一个 KAT Response JSON。只在 `status="success"` 时读取 `result`；失败时读取 `error`，以及存在时的 `log_path` 或 `test_report_path`。不要从终端文本、日志或 pytest 输出推断成功。

## 查看帮助

```text
kat --help
kat <命令> --help
```

用于确认固定 CLI 路由和外层参数。Workflow 的业务参数不在 `kat run --help` 中；必须先执行 `kat inspect --pack <名称>`，再按 Response 的 `parameters` 在 `--` 后传入。

## 发现或检查 PACK

```text
kat inspect
kat inspect --pack <PACK名称> [--pack-dir <PACK目录> ...]
```

- 第一条发现当前可用的 PACK；成功 `result.packs` 为空也是有效结果。
- 第二条检查一个精确 PACK；`--pack-dir` 可重复，每个目录必须直接包含 `pack.toml`。成功 Response 的 Workflow 用途和 `parameters` 决定选择哪个入口以及如何传参。

KAT 不提供来源 inspection。来源文件、数据库 selector 和其他来源输入由选定 Workflow 的参数显式表达，其准入由 PACK-owned Datasource Provider 在执行时完成。

## 执行 Workflow

```text
kat run --pack <PACK名称> --workflow <Workflow名称> \
  [--pack-dir <PACK目录> ...] -- \
  <来自 inspection.parameters 的 Workflow 参数>
```

用于执行一个已检查的 Workflow。`--` 后的 token 原样交给 Workflow Input Compiler；只传 inspection 明示的 option，且不要包含秘密，因为这些参数可能写入 Operation log。成功后从 `result.run_id` 和 `result.outputs` 取得唯一可查询 Run 及其 Output 元数据。

例如，若 Workflow 的 inspection 明示 `--source-path` 和 `--limit`：

```text
kat run --pack example --workflow analyze --pack-dir <PACK目录> -- \
  --source-path <本地来源路径> --limit 20
```

## 查询既有 Run

```text
kat query --run <Run ID> --sql <一条只读 SQL>
```

SQL 只能引用所选 Run 的 `output.<name>` relation。构造 SQL 前必须已有 Output 名称与 columns：优先使用刚完成的 `kat run` 成功 Response 中的 `result.outputs`，也可以使用用户提供的同等元数据。只有 Run ID 时没有受支持的 Output 发现命令；应请求用户补充元数据并停止，不猜表名、不读取 Run 内部文件。

在 SQL 中自行加入投影、过滤、聚合与排序；明细查询在发送前显式写入 `LIMIT`。KAT 不自动添加规模或时间限制。成功 Response 的 `result` 精确包含：

- `format`：当前固定为 `ndjson`；
- `path`：Runtime 直接写出的单个 NDJSON 文件；
- `columns`：有序列描述，每个 NDJSON 行都是一个使用查询列名的 JSON object。

零行查询产生空文件，行不会内联到 Response。不要读取 Run 内部 Output 文件，也不要把实际执行失败包装成部分成功。

## 测试一个 PACK

```text
kat test --pack-dir <PACK目录>
kat test --pack-dir <PACK目录> --test <pytest node ID> [--test <pytest node ID> ...]
```

用于在生产执行平面运行该 PACK 的 pytest。`--pack-dir` 是一个直接包含 `pack.toml` 的精确目录，不使用 PACK 名称。测试 fixture 用普通来源文件、配置和临时路径构造 Provider；`kat_run` 只接收 Workflow 和 arguments。成功 `result.summary` 是测试结论；失败时引用 Response 的 Diagnostic，以及存在时的 `test_report_path` 和 `log_path`。测试或诊断失败不授权修改 PACK。
