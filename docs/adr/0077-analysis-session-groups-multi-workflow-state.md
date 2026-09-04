---
status: accepted
---

# Analysis Session 统一承载多 Workflow 分析状态

一次多 Workflow 分析使用一个 Analysis Session 统一归属多个独立 Run、可复用的 Datasource materialization 与候选执行的临时工作数据。Session 与其中每个 Run 分别使用独立 UUIDv7 身份，但沿用同一种 ID 格式和生成机制；Session 只统一身份、寻址和整体生命周期，不把不可变 Run Output、来源物化与 scratch 变成同一种事实。

Session 不归属于 PACK；每个 Run 精确属于一个 Session，并继续记录自己选择的 PACK 与 Workflow。同一 Session 可以包含不同 PACK 的 Runs，但 Runtime 每次仍只挂载当前 Run 选择的 PACK，不加载其他 PACK 的代码或建立 PACK dependency。

`kat run` 可以通过可选的 `--session <session-id>` 继续一个已发布 Session。未提供该参数时，每次都创建新 Session；KAT 不提供隐式 current/last Session、单独的 create 命令、全局 Session registry 或跨 Data Home 查找。显式 Session 缺失、正在删除或身份标记损坏时执行失败，绝不静默创建同名 Session。

公开存储布局为：

```text
KAT_DATA_HOME/
└── sessions/
    └── <session-id>/
        ├── session.json
        ├── materializations/
        │   └── <source-stem>/
        ├── scratch/
        │   └── <candidate-id>/
        └── runs/
            └── <run-id>/
                ├── manifest.json
                └── outputs/
```

`sessions/.leases/<session-id>.lock` 与 `sessions/.deletions/<session-id>/` 是删除协调使用的私有固定位置，不属于公开 Session 内容或发现面。前者在首次候选执行时建立；未发布 Session 失败时可以清理，已发布过的 Session 即使删除后也永久保留其空 lease 文件，因为 UUIDv7 不复用，而安全删除同名锁文件需要额外全局协调。首版接受这些小型文件累积，不把它们当成 Session registry。

`session.json` 精确只含 `session_id`，是 Session 的不可变身份和公开标记，不维护 Runs、materializations、PACK、Workflow、当前运行或状态。Session 解析只接受当前 Data Home 下 `sessions/<session-id>` 的规范 UUIDv7 直接普通目录、精确匹配目录名的普通标记文件和三个固定普通子目录；symlink、junction、路径逃逸、身份不一致、缺失与损坏均 fail closed。

未提供 `--session` 的首次执行从一开始就在最终 `sessions/<session-id>` 路径形成未发布候选，以免目录移动破坏物化中可能存在的绝对自引用。CLI 生成彼此独立的 Session ID 与 Run candidate ID，完成 Runtime、Outputs、Operation log 和 scratch 清理后先原子发布首个 Run Manifest，最后以 no-replace 方式原子发布 `session.json`；公共读取必须先通过 Session 标记，因此只有最后一步成功后 Session 与首个 Run 才同时可见，success Response 才返回两个 ID。

首次执行任一步失败都不返回 Session ID 或 Run ID，并清理整个未发布目录。进程被强制终止或清理失败可能留下没有合法 `session.json` 的候选残留；它不是 Session，公共操作不扫描、发现或读取它，首版也不增加自动 GC。

已有 Session 上的每次执行使用新的 candidate ID 隔离 `scratch/<candidate-id>` 与 `runs/<candidate-id>`。只有 Runtime、Outputs、Operation log 和 scratch 清理全部成功后，CLI 才以最终 `manifest.json` 发布该 candidate，使其 ID 成为 Run ID；失败时删除本次 scratch 和未发布 Run candidate，不返回 Run ID，也不回滚已经独立完整发布的来源物化。Scratch 清理是 Run 发布门，清理失败时不发布 Run。

Run Manifest 同时记录 `session_id` 与 `run_id`，并继续记录 PACK、Workflow、有效输入和 Outputs。Run 的公共地址改为 `(session_id, run_id)`：`kat run` success Response 同时返回两者；所有读取既有 Run 的操作必须显式提供二者，例如 `kat query --session <session-id> --run <run-id> --sql ...` 与 `kat inspect workflow --session <session-id> --run <run-id>`。解析必须验证两个路径参数、Session 标记、Run 目录和 Manifest 内身份全部一致；KAT 不扫描其他 Sessions，不维护 `run_id → session_id` 索引，也不保留只凭 Run ID 的兼容寻址。

`kat inspect session --session <session-id>` 的 success result 精确为：

```json
{
  "session_id": "...",
  "runs": [
    {
      "run_id": "...",
      "pack": "...",
      "workflow": "...",
      "outputs": {
        "main": {
          "columns": [{"name": "...", "type": "..."}],
          "row_count": 0
        }
      }
    }
  ]
}
```

每个 `outputs` 精确复用 Run success 的公开 inventory 形状，Runs 按 Run ID 稳定排序；结果不包含 inputs、materializations、scratch 或物理路径，首版也不分页。Inspection 只枚举一次当前 `runs/`：当时没有 Manifest 的候选被忽略，枚举后新发布的 Run 可以不出现在本次结果中；每个已经选中的 Run 都复用 Query 的同一个 published-Run resolver，严格验证两个身份、Manifest 及每个声明 Output 的直接普通文件，任一损坏时整体失败而不返回部分 inventory。正常发布的 Session 至少包含一个有效 Run。

`ctx.datasource_root` 指向当前 Session 的 `materializations/`，`ctx.scratch_root` 指向当前执行的 `scratch/<candidate-id>/`；两者都是受同一调用期 Execution Lease 约束的普通 `Path`。Context 不增加 `session_id` 或 `session_root` 属性，Runtime 也不把整个根作为请求字段，但这只是收窄正常作者接口：受信任 PACK 仍可能沿父目录越界，KAT 不是 Python 文件系统沙箱，Run 文件私有性与跨 PACK 只读复用都是作者合同而非安全强制。

需要复用来源的 Provider 从 Workflow 明确传入的来源路径取得 `Path(source).stem`，只去掉最后一个后缀，并以其作为 `materializations/<source-stem>`；Workflow 不扫描目录来猜测来源。Provider 作者合同要求拒绝空名称、`.`、`..`、路径分隔符、控制字符、Windows 非法字符、尾随点或空格及大小写不敏感的 Windows device name，不自动清洗、归一化或消歧；Runtime 无法知道哪个参数是来源，因此不代替 Provider 执行这项校验，首方与 reference Provider 必须示范并测试相同规则。同一分析中的名称唯一性和大小写碰撞由用户保证。

同一 Session 内，某个 source stem 首次以 staging、完整关闭与校验、no-replace 方式成功发布后，绑定该 Session 的首次物化且不可原位替换。每次复用都由 Provider 自己 `dp.open()` 并校验所需 relations、columns 与版本合同；目录存在本身不是命中证明。交给 `dp.open(root=...)` 的 materialization 根必须是 `materializations/` 下的直接普通目录，每个 relation 必须是该根下的直接普通 Parquet 文件；根或 relation 是 symlink、junction、任何 Windows reparse point，或解析后逃出该根时均拒绝且不跟随，避免把 Session 外的可变文件误作当前 Session 已固定的来源事实。已有物化打不开、损坏或合同不兼容时当前执行失败，只能换 source stem 或新建 Session 后重建；任何 PACK 都不得删除、覆盖或修复原槽位。原始来源文件随后变化也不会刷新同一 Session 的物化，分析新内容必须使用新 stem 或新 Session。这里有意让 Session 内事实固定优先于旧 cache 的原位修复：没有来源 hash、snapshot 或 provenance 时，重新 decode 无法证明仍对应首次事实。

原生 decoder 在每个已发布 Parquet relation 的 Arrow Schema metadata 中写入 bytes key
`kat.materialization.version`；文本 Ftrace 当前值为 `text-ftrace-v1`，Hitrace 当前值为
`hitrace-v1`。对应 Python wrapper 以
`MATERIALIZATION_VERSION_METADATA_KEY` 与 `MATERIALIZATION_VERSION` 导出合同常量。
Provider 命中时必须拒绝未知 relation，验证每个实际 relation 的完整列、物理类型与
nullability，并逐一
检查该 metadata；缺失、未知值或仅 Schema 相同都不构成兼容。版本只表达 decoder
materialization 合同，不是来源 provenance、内容 hash 或 Session registry。

KAT 不建立 materialization registry、来源 provenance、生产者身份或强制只读 API。首次发布者只定义该槽位的字节事实，不取得以后原位重建的特殊权限；跨 PACK 共享依赖受信任作者之间的显式数据合同和集成测试。`dp.write()` 或具体 decoder 只提供候选事务与 no-replace 发布；竞争失败的 Provider 自行打开并校验胜者，兼容则采用，不兼容则使当前执行失败且不破坏胜者。自定义 Provider 不采用这一发布协议时，KAT 不承诺并发安全。

Run Manifest 中记录的来源类 effective input 只表示调用参数，不证明共享物化字节的 provenance；命中既有 source stem 时，Guide 与 Analysis Result 不得仅凭该输入声称物化来自本次传入路径。

同一 Session 允许多个普通操作并发，不使用 exclusive Session 锁把 Runs 串行化。Run candidate 和 scratch 由 candidate ID 隔离；竞争同一 source stem 的生产方各自生成完整候选，再以 no-replace 决出唯一胜者。普通 Run、Query、按 Run 的 Workflow inspection 与 Session inspection 在操作及最终 Response 发布期间持有同一个 Session shared lease，只用于和整体删除协调，不妨碍彼此并发。该 lease 协调仍存活的 KAT CLI 操作，不是进程沙箱；若 CLI 被强制终止而 Runtime 子进程成为孤儿，删除对 rename 或递归删除的系统错误 fail closed，但首版不增加 parent-death 或孤儿进程控制协议。

`kat session delete --session <session-id>` 是唯一删除入口，永久删除该 Session 的 Runs、Outputs、materializations 和 scratch；它不创建 Operation log，success result 精确只有 `{"session_id":"<session-id>"}`，failure 不含 result。删除使用固定私有 lease 文件取得 non-blocking exclusive lease；有普通操作占用时立即失败且不修改 Session。取得 lease 后再次严格验证：只有 Session 存在时，将它 no-replace 移到固定 `.deletions/<session-id>` 后递归删除；只有 tombstone 存在时继续删除；二者都不存在时报 Not Found，二者同时存在则按损坏 fail closed。递归删除失败仍返回 failure，普通操作不读取 tombstone，并把已移走的 Session 视为不存在或正在删除。

首次移动前必须完整验证 Session；递归删除失败后 tombstone 可能只剩部分内容，因此重试只验证 `.deletions/<规范 UUIDv7>` 是当前 Data Home 内的直接普通目录且没有 symlink、junction 或路径逃逸，不再要求其中保留完整 Session 标记和三个子目录。该收窄只适用于固定 tombstone 的续删，不能放宽公开 Session resolver 或首次删除准入。

所有递归删除只接受经验证的当前 Data Home 内精确 Session 与 tombstone 目标。删除不移除位于 Session 外的 Operation logs 或 Query Results；它们没有反向索引，调用方必须知道删除 Session 后这些独立产物仍可能保留。Session 首版不按 TTL、访问时间、磁盘配额或其他策略自动回收，也不提供单 Run 删除。

`kat test` 为每个 pytest test 建立隔离的测试 Session：同一 test 中多次 `kat_run` 共享 materializations，每次调用使用独立 candidate、scratch 和 Outputs；不同 test 完全隔离。测试 Session 不发布生产 `session.json` 或生产 Run，失败现场是否由 pytest 临时目录保留不改变生产 Session 合同。

Query Results 与 Operation logs 继续保留在现有 Data Home 全局目录，Run Output 也不会自动传给下一个 Workflow。多个来源必须由 Workflow 参数显式选择，不扫描或猜测；Guide 可以让后续 Workflow 复用成功 Response 中的 `session_id`，但不能因此自动形成 DAG、隐式 Workflow 串联或扩大来源授权。

这是一项早期破坏性变更。新版本只读取 `sessions/<session-id>/...`；既有顶层 `runs/` 与 `datasources/<pack>/` 不迁移、不扫描、不删除，也不提供兼容 locator。切换 Data Home 后，原 Session 地址在新 Data Home 中无效。

## 与既有决定的关系

本决定完整取代 ADR-0069 的 PACK 私有可替换 cache 身份与无并发协调合同；Provider 仍拥有来源语义与准入检查。

下列决定只被局部取代，未列出的其余边界继续有效：

| 既有决定 | 本决定取代 | 继续有效 |
| --- | --- | --- |
| ADR-0001、ADR-0002、ADR-0010 | Data Home 顶层 `runs/<run-id>`、Run candidate 与 Run resolver 的物理寻址 | CLI/Runtime 所有权、Manifest 发布门、失败候选不是 Run及 Data Home 其他边界 |
| ADR-0008、ADR-0036 | Run success 只返回 Run ID、`kat query --run` 与单 ID 查询定位 | Output inventory、最少证据查询、Query 不创建 Run及 Skill-first JSON |
| ADR-0019 | `run_workflow` 的 candidate/root 字段和 `query_run` 的 Run 定位 | 封闭 operation-specific typed IPC 与单进程单操作 |
| ADR-0027 | 跨 PACK source fact 复用只能通过 Dataset 表达、没有 Session 共享物化范围的部分 | PACK 自包含、无代码 dependency、每次只挂载当前 PACK |
| ADR-0016、ADR-0032、ADR-0063、ADR-0075 | 每次 `kat_run` 的独立执行根、PACK 私有 `ctx.datasource_root`、Context 根数量与测试复用范围 | pytest fixture、真实 Workflow 执行、显式调用期 Context、普通 PACK-owned Provider、Dataset 退役与查询边界 |
| ADR-0035 | `kat run`、`kat query` 与按 Run inspection 的单 ID 参数合同 | Workflow argument 编译与 Runtime 参数语义 |
| ADR-0055、ADR-0056 | Output 公开引用与 canonical Run path 从 `(run_id, output_name)` 改为 `(session_id, run_id, output_name)` | 可移植 Output name、不可变 Output 与可信本地 IPC 分工 |
| ADR-0074 | `kat inspect workflow --run` 的单 ID 定位 | 当前 PACK declaration、Guide 与 Provider inspection |

ADR-0060 的 Data Home 选择合同完整保留；ADR-0076 的 `dp.write()` staging、完整校验和 no-replace 发布合同完整保留，并作为 source stem 并发竞争的底层原语。

## 明确不做

首版不增加用户传入的 datasource/session 物理根、Run Output 自动输入、可变 Session 清单、materialization registry、生产者认证、文件系统沙箱、来源 hash、自动消歧、来源变更检测、DAG、隐式执行、Session list/current、单 Run 删除、TTL、GC、配额、旧数据迁移，或把 Query Results 与 logs 搬进 Session。
