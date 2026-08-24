# KAT 命令速查

用户只需用自然语言说明目标。以下命令由 Agent 使用；始终以 `SKILL.md` 所选平台 Payload 的绝对 `kat` 路径替代示例中的 `kat`。

除 `--help` 外，每次命令调用的 stdout 都是一个 KAT Response JSON。只在 `status="success"` 时读取 `result`；失败时读取 `error`，以及存在时的 `log_path` 或 `test_report_path`。不要从终端文本、日志或 pytest 输出推断成功。

## 查看帮助

```text
kat --help
kat <命令> --help
```

用于确认固定 CLI 路由和外层参数。Source 与 Workflow 的业务参数不在外层 help 中；必须先执行 `kat inspect --pack <名称>`，再按 Response 中对应入口的 `parameters` 在 `--` 后传入。

## 检查 Dataset 或发现 PACK

```text
kat inspect --dataset <Dataset目录>
kat inspect
kat inspect --pack <PACK名称> [--pack-dir <PACK目录> ...]
```

- 第一条只读检查 Dataset；成功 `result.sources` 按 PACK/Source 显示 External Binding 或 Materialized Source。只有 Materialized Source 带 `tables` 与 columns。
- 第二条发现当前可用的 PACK；成功 `result.packs` 为空也是有效结果。
- 第三条检查一个精确 PACK；`--pack-dir` 可重复，每个目录必须直接包含 `pack.toml`。成功 Response 的 `source_guide`、`sources` 和 `workflows` 决定数据如何接入、可执行哪些分析以及如何传参。Inspection 不调用 Source Entry，也不枚举动态表。

## 建立 External Binding

```text
kat bind --pack <PACK名称> --source <Source名称> --dataset <Dataset目录> \
  [--replace] [--pack-dir <PACK目录> ...] [-- <Source参数>]
```

用于把一个 PACK Source 的外部绑定配置保存到明确的 Dataset。调用前先执行 `kat inspect --pack`，阅读 `source_guide`，并只使用对应 `sources[].parameters` 明示的 option。`--pack-dir` 必须位于 `--` 之前。

`kat bind` 会校验 Source 合同和参数，但不会调用 Source Entry、读取文件、连接数据库、验证凭据或枚举表。成功后从 `result.path`、`result.pack`、`result.source` 和 `result.kind="external"` 取得 Binding 身份。同一 PACK/Source 已有 Binding 时默认失败；只有用户明确要求完整替换时才传 `--replace`，本次 Source 参数不会与旧参数合并。

External Binding 会在 `bindings.json` 中明文保存 `--` 后的原始 token 和绑定时的绝对工作目录，其中可能包含密码、Token 或 DSN。KAT 不提供加密、脱敏或保密保证；在让用户确认这种存储方式之前，不要替用户把秘密写入 Binding。

## 物化一个 Source

```text
kat materialize --pack <PACK名称> --source <Source名称> --dataset <Dataset目录> \
  [--table <表名> ...] [--replace] [--pack-dir <PACK目录> ...] \
  [-- <Source参数>]
```

用于读取 Source Provider，并把结果发布为 Dataset 中的 Materialized Source。省略 `--table` 表示发布该 Source 当前提供的全部表；重复 `--table` 只发布明确子集。已有任一 Binding 时必须由用户明确授权 `--replace`。

本次提供 Source 参数时使用本次参数；未提供参数且当前已有 External 或 Materialized Binding 时，重放其中保存的 REDO 配方；目标尚未绑定时使用空参数，由当前合同补齐默认值，缺少必填参数则失败。`--table` 只决定本次发布的表，不写入 REDO 配方，因此以后不带 `--table` 的重做会恢复 Source 当前提供的全部表。成功后从 `result.path`、`result.pack`、`result.source`、`result.kind="materialized"` 和排序后的 `result.tables` 取得事实。表 Schema 继续通过 `kat inspect --dataset` 读取。

## 执行 Workflow

```text
kat run --pack <PACK名称> --workflow <Workflow名称> \
  [--dataset <Dataset目录>] [--pack-dir <PACK目录> ...] -- \
  <来自 inspection.parameters 的 Workflow 参数>
```

用于执行已检查的 Workflow。`--dataset` 可省略；省略表示本次没有 Dataset-backed Source Configuration，KAT 不猜测默认 Dataset。生产 `kat run` 不接受临时 Source 参数：需要外部配置时先执行 `kat bind`，需要本地事实时先执行 `kat materialize`。Workflow 首次查询某个 Source 时才解析对应 Binding；任意 PACK 的 External Binding 都可执行，但对应 PACK 必须被唯一发现，必要时用 `--pack-dir` 补充候选。

`--` 后的 token 只交给 Workflow Input Compiler；只传 inspection 中对应 `workflows[].parameters` 明示的 option，且不要包含秘密，因为这些参数可能写入 Operation log。成功后从 `result.run_id` 和 `result.outputs` 取得唯一可查询 Run 及其输出元数据。

## 查询 Dataset 或既有 Run

```text
kat query --run <Run ID> [--pack-dir <PACK目录> ...] --sql <一条 SQL>
kat query --dataset <Dataset目录> [--pack-dir <PACK目录> ...] --sql <一条 SQL>
```

两种目标必须且只能选择一个。Run 模式注册 `output.*`，并在关联 Dataset 当前有效时追加其中的 Sources；Dataset 模式注册 Dataset Sources。External 与 Materialized Source 使用同一套 SQL 界面：External Binding 在首次引用时按需执行对应 PACK，Materialized Source 直接读取本地 Parquet。查询 External Binding 时，确保对应 PACK 被唯一发现，必要时传入可重复的 `--pack-dir`。跨 PACK 表使用带引号的三段式名称，例如 `"kat-kernel".raw_smaps.mappings`。

SQL 只访问本次问题需要的列和行，并自行加入投影、过滤、聚合与排序；明细查询在发送前显式写入 `LIMIT`。成功 `result.columns` 与 `result.rows` 是分析证据；不要读取 Dataset/Run 内部文件、假设 KAT 会自动添加限制，或把实际执行失败包装成部分成功。

构造 SQL 前必须已有 Run Output 名称与 columns：优先使用刚完成的 `kat run` 成功 Response 中的 `result.outputs`，也可以使用用户提供的同等元数据。只有 Run ID 时没有受支持的 Run Output 发现命令；应请求用户补充元数据并停止，不猜表名、不读取 Run 内部文件。

## 测试一个 PACK

```text
kat test --pack-dir <PACK目录>
kat test --pack-dir <PACK目录> --test <pytest node ID> [--test <pytest node ID> ...]
```

用于在生产执行平面运行该 PACK 的 pytest。`--pack-dir` 是一个直接包含 `pack.toml` 的精确目录，不使用 PACK 名称。成功 `result.summary` 是测试结论；失败时引用 Response 的 Diagnostic，以及存在时的 `test_report_path` 和 `log_path`。测试或诊断失败不授权修改 PACK。
