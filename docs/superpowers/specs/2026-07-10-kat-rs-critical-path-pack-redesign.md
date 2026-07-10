# kat-rs 通用关键路径示例 Pack 重构设计

## 1. 文档目的与结论

本文定义 `packs/openharmony-critical-path` 的重构方案。目标不是增加新的 Pack runtime 能力，而是让现有示例真正体现 `@workflow`、`@fact`、`@compute` 的职责边界，并完整实现 `docs/critical-path.strategy.md` 描述的关键路径策略。

本文经确认修订 `2026-07-09-kat-rs-short-lived-cli-python-pack-rewrite-design.md` 中“`@compute` 只接收和返回单个 DataFrame”的约束：fact 与 compute 之间仍使用 DataFrame 传递事实，但 compute 可以接收显式 FactProvider、维护内存遍历状态并返回持有两个 DataFrame 的 `CriticalPathResult`。workflow 对 runtime 的 `dict[str, DataFrame]` 返回合同不变；其他 Pack 和 runtime 合同不因本文扩展。

核心结论：

- 公共分析入口是通用 `critical_path` workflow，输入为 `root_itid`、`start_ts`、`end_ts`、`max_depth` 和 `min_segment_ms`。
- 微信首帧保留为独立场景 workflow，只负责选择根线程和时间窗口，然后复用同一个通用 compute。
- 所有事实来源、SQL 和来源字段解码属于 fact。
- 图遍历、逐层查询循环、遍历状态、分类、上下文传播和终止判断属于 compute。
- workflow 只负责组装 fact provider、调度 compute 和返回 artifacts，不维护算法循环。
- 正式输出仍只有 `path_nodes` 和 `path_edges` 两个 DataFrame artifact。

## 2. 当前问题

现有示例存在三个直接问题：

1. 唯一公共 workflow 是 `wechat_first_frame_critical_path`，让通用关键路径能力依附于微信首帧窗口。
2. `compute/critical_path.py` 接收 `kat` 并用一段递归 SQL 同时完成事实读取、图遍历、分类和输出组装；现有 facts 未参与算法，装饰器边界名存实亡。
3. `facts/trace_streamer.py` 以某个数据生产工具命名，泄漏来源实现，没有表达线程、调度和调用栈等领域事实。

现有算法也没有完整落实策略文档：它没有按状态时间序列从窗口终点向前分析，没有在等待到 Runnable 的转换处选择 waker，也没有完整处理调度证据、IO 线程、阻塞上下文下沉、环和全部不确定性。

## 3. 目标

本次重构必须做到：

1. 提供与具体应用和窗口类型无关的通用关键路径 workflow。
2. 保留微信首帧作为薄场景适配器，禁止场景知识进入通用 compute。
3. 逐层、按当前线程和当前窗口查询事实，不预加载整个 trace。
4. 由 compute 维护 frontier、时间游标、visited edges、路径节点、路径边、阻塞上下文和所有判断规则。
5. 完整实现策略文档中的状态、调度、唤醒、IO、阻塞上下文、终止和不确定性规则。
6. 保持两个正式 artifact，并让事实、推断和不确定性在字段上可区分。
7. 使用 fake fact provider 独立验证 compute，不要求 compute 测试加载 dataset。

## 4. 非目标

本次不做：

- 不修改短命 CLI、Python worker、dataset catalog 或 artifact runtime。
- 不新增通用 fact registry、动态插件、查询 DSL 或通用图引擎。
- 不引入批量预取、缓存或跨 run 状态。
- 不实现来源数据没有提供的 Binder、锁或 block I/O 细节；缺失证据必须记录为不确定性。
- 不把 `critical-path.strategy.md` 直接做成可执行 DSL。
- 不扩大到其他 Pack 或无关 runtime 重构。

## 5. 装饰器职责

### 5.1 `@fact`

Fact 拥有全部事实访问：

- 知道 DataFusion 表名和来源 schema。
- 执行 SQL。
- 做来源字段连接和规范化，例如把 `thread_state.arg_setid` 经 `args`、`data_dict` 解码为 `iowait` 和 `blocked_caller`。
- 只返回原始或规范化事实 DataFrame，不选择关键片段，不解释状态，不判断关键路径。

### 5.2 `@compute`

Compute 拥有全部领域计算：

- 维护逐层查询循环和 run-local 遍历状态。
- 决定下一次需要什么事实，并通过显式 fact provider 获取它。
- 对状态片段排序、裁剪、选择和分类。
- 维护依赖图、环检测、深度限制和阻塞上下文。
- 生成 `path_nodes` 和 `path_edges` DataFrame。

Compute 不接收 `kat`，不写 SQL，不知道表名或 TraceStreamer/SQLite。本文将原设计中“compute 只接收和返回 DataFrame”的窄定义收敛为：compute 不直接访问 dataset；它可以通过显式 fact provider 按需读取事实，并维护本次计算的内存状态。该修订只影响 Pack 内部 fact/compute 合同，不改变 workflow 的 runtime 返回合同。

### 5.3 `@workflow`

Workflow 只负责组合：

- 把绑定了 `kat` 的 fact 函数组装为 `FactProvider`。
- 构造 compute 请求并调用 `extract_critical_path`。
- 把 `CriticalPathResult` 转成 `{"path_nodes": ..., "path_edges": ...}`。

Workflow 中不得出现 SQL、状态分类、递归、frontier 或 visited 集合。

## 6. Pack 目录

```text
packs/openharmony-critical-path/
  pack.py
  workflows/
    critical_path.py
    first_frame.py
  facts/
    threads.py
    scheduling.py
    callstacks.py
    frames.py
  compute/
    critical_path.py
    models.py
```

删除 `facts/trace_streamer.py`。文件名只描述领域能力，不描述来源工具或物理存储。

## 7. Fact 合同

### 7.1 线程事实

`facts/threads.py` 提供：

```text
thread_metadata(kat, itid) -> DataFrame
thread_state_segments(kat, itid, start_ts, end_ts) -> DataFrame
```

`thread_metadata` 返回 `itid`、`tid`、`thread_name`、`pid`、`process_name`。

`thread_state_segments` 返回与窗口相交的原始状态片段，至少包含：

```text
itid, ts, dur, state, cpu, arg_setid, iowait, blocked_caller
```

Fact 负责解码 `iowait` 和 `caller`；compute 负责裁剪片段、解释 `D-IO`/`D-NIO` 和形成阻塞结论。

### 7.2 调度与唤醒事实

`facts/scheduling.py` 提供：

```text
wakeup_edges(kat, target_itid, start_ts, end_ts) -> DataFrame
sched_slices(kat, itid, start_ts, end_ts) -> DataFrame
```

`wakeup_edges` 返回实际 `sched_wakeup%` 记录：

```text
wakeup_ts, target_itid, waker_itid, name
```

`sched_slices` 返回窗口内实际调度片段，至少包含：

```text
itid, ts, dur, ts_end, cpu, priority, end_state
```

### 7.3 调用栈事实

`facts/callstacks.py` 提供：

```text
callstack_slices(kat, itid, start_ts, end_ts) -> DataFrame
```

它返回覆盖请求窗口的调用栈片段。Fact 不把最长调用栈解释为关键路径；compute 只能把调用栈作为状态结论的补充证据。

### 7.4 场景锚点事实

`facts/frames.py` 提供：

```text
first_frame_window(kat, app_name) -> DataFrame
```

它只返回微信场景所需的 `root_itid`、`start_ts` 和 `end_ts`。默认应用名仍可为 `.tencent.wechat`，但该默认值只存在于场景 workflow。

## 8. Compute 数据结构

`compute/models.py` 只定义当前算法需要的结构：

- `FactProvider`：线程、状态、唤醒、调度和调用栈回调合同。
- `TraversalFrame`：当前 `itid`、窗口、depth、父节点和继承的阻塞上下文。
- `TraversalState`：frontier、visited wakeup edges、已生成节点、已生成边和稳定 ID 计数器。
- `PathNode`、`PathEdge`：输出行模型。
- `CriticalPathResult`：最终两个 DataFrame。

不建立可复用图框架。frontier 和 visited 的具体容器由算法需要决定，生命周期只覆盖一次 compute 调用。

Compute 将最终行模型写入一个只承载内存结果的 DataFusion `SessionContext`，生成 nodes 和 edges DataFrame。该 context 不注册 dataset 表，也不成为新的事实来源。

## 9. Workflow

### 9.1 通用入口

通用公共入口为：

```python
@workflow(title="Critical path", description="Extract a critical path from a root thread and time window")
def critical_path(
    kat,
    root_itid: int,
    start_ts: int,
    end_ts: int,
    max_depth: int = 8,
    min_segment_ms: float = 0.1,
): ...
```

函数只绑定 facts、调用 `extract_critical_path` 并返回两个 artifacts。

### 9.2 微信首帧入口

`wechat_first_frame_critical_path` 保留为独立 workflow：

1. 调用 `first_frame_window` fact。
2. 找不到目标时返回 `target_not_found` 终止结果。
3. 找到目标时使用相同 FactProvider 和通用 compute。

它不复制关键路径逻辑，也不向 compute 传入应用名或 frame 概念。

## 10. 关键路径算法

### 10.1 初始化

Compute 校验输入后创建：

```text
TraversalFrame(root_itid, start_ts, end_ts, depth=0)
```

算法从窗口终点向起点逆向分析。根线程为 `depth=0`；每进入一层 waker，depth 加一。

### 10.2 逐层事实查询

每处理一个 frame：

1. 查询线程元数据和窗口内状态片段。
2. 对状态片段排序并裁剪到当前窗口。
3. 从 `end_ts` 向 `start_ts` 遍历状态。
4. 仅在规则需要时查询 sched、wakeup 或 callstack 事实。

事实查询始终限定为当前线程和当前窗口，不预加载全 trace。

### 10.3 状态处理

- `Running`：查询重叠的 `sched_slice` 和 `callstack`。只有状态与调度证据一致时才给出 `self_running`；缺调度证据时为 `unknown` 并记录 `missing_sched_evidence`。调用栈只作为补充事实。
- `R`/`R+`：记录 `scheduler_wait`。若它之前紧邻等待片段，则在等待到 Runnable 的转换处查找 waker。
- `S`：存在有效 waker 时记录 `waiting_for_waker`；缺 waker 时保持未知并记录不确定性。
- `D-IO` 或 `iowait=1`：记录 `io_block`。
- `D`/`D-NIO`：存在非 IO `blocked_caller` 时记录 `non_io_block`；证据不足时保持未知。
- 其他状态：记录 `unknown`，不生成依赖猜测。

### 10.4 Waker 选择与递归

当等待片段进入 Runnable：

1. 依赖窗口为等待片段开始到 Runnable 开始。
2. 查询该窗口内目标线程的 wakeup facts。
3. 选择不晚于 Runnable 起点、时间最接近起点的记录。
4. 若同一最终时间点存在不同 waker，记录 `ambiguous_waker`，不任选来源。
5. 对唯一 waker 记录事实边，并创建 `depth + 1` 的 waker frame。
6. 子层完成后继续分析当前线程更早的状态。

跨线程 wakeup edge 从 waker 节点指向被唤醒线程的等待节点。同一 depth 的片段按时间从早到晚生成 sequence edge。

### 10.5 阻塞上下文

上层 `D` 类状态的 `blocked_caller` 进入子层 frame 时作为继承上下文。子层节点同时记录：

- 自己查询到的 `blocked_caller`。
- 继承上下文的来源节点和 caller。

继承信息只表达“该子路径处于上层阻塞链中”，不能改写成子线程自己的阻塞事实。

### 10.6 IO 与 IRQ

初始 IO 线程集合按策略文档精确匹配：

```text
fsverity
cdecrypt
erofs_unzipd
fsignature
hmfs
wk:0/0/0
wk:2/1/0
wk:0/-20/0
```

`hmfs_txn` 明确排除。被等待依赖的 waker 命中 IO 线程集合时，上层等待节点分类为 `io_block`，并继续按普通 waker 分析。waker 为 `udk-irq` 时生成可引用的终止节点和事实边，标记 IO 证据，但不继续递归。

### 10.7 最小片段

`min_segment_ms` 只过滤不参与依赖关系的短噪声片段。以下短片段必须保留：

- wakeup edge 的端点。
- 等待到 Runnable 的状态转换。
- 阻塞、终止或不确定性证据。

过滤不得改变 waker 选择或遍历结论。

### 10.8 终止条件

算法在以下条件停止当前分支：

- 到达 `start_ts`。
- 当前线程没有覆盖窗口的状态片段。
- 找不到有效 waker。
- waker 为 `udk-irq`。
- 达到 `max_depth`。
- 唤醒边 `(waiter_itid, waker_itid, wakeup_ts)` 重复形成环。

除输入或 fact 合同错误外，这些都是成功运行中的领域结果。

## 11. 分类与证据

分类枚举：

```text
self_running
scheduler_wait
waiting_for_waker
io_block
non_io_block
unknown
```

规则保持保守：

- 状态、时间、线程、调度、wakeup 和调用栈是事实。
- `classification` 是 compute 基于事实做出的推断。
- `uncertainty` 与 `termination_reason` 明确说明证据缺口和停止原因。
- 所有 wakeup edges 必须能回查实际 `instant` 记录。
- 禁止把最长片段或最长调用栈直接写成根因。

## 12. 输出合同

### 12.1 `path_nodes`

```text
node_id
depth
itid, tid, thread_name, pid, process_name
window_start_ts, window_end_ts
segment_start_ts, segment_end_ts, dur, state
classification
sched_cpu, sched_priority, callstack_name
blocked_caller
blocking_context_node_id, inherited_blocked_caller
confidence, uncertainty, termination_reason
```

`node_id` 按确定的遍历顺序生成，使相同输入和相同 facts 得到稳定结果。

`missing_state` 或 `target_not_found` 终止节点允许事实字段为空，但必须保留 `node_id`、classification、uncertainty 和 termination reason。

### 12.2 `path_edges`

```text
edge_id
from_node_id, to_node_id
from_itid, to_itid
parent_depth, child_depth
wakeup_ts
edge_type
confidence
reason
```

`edge_type` 只包含：

```text
sequence
wakeup
```

sequence edge 从较早片段指向较晚片段；wakeup edge 从 waker 节点指向等待节点。两类边都来自事实关系，`confidence` 为 `fact`。

### 12.3 终止原因

稳定终止枚举：

```text
window_start_reached
missing_state
missing_waker
udk_irq
max_depth
cycle_detected
target_not_found
```

`ambiguous_waker` 是 uncertainty；该情况下不生成 wakeup edge。

## 13. 错误处理

以下情况使 Pack Run 失败：

- `start_ts >= end_ts`。
- `max_depth` 或 `min_segment_ms` 为负数。
- Fact SQL 执行失败。
- Fact 返回值缺少必需列或类型不兼容。
- Compute 无法构造输出 DataFrame。

Fact 错误必须带上 capability name、`itid` 和请求窗口，并保留原始异常作为 cause，供 runtime manifest 记录 traceback。

以下情况不使运行失败：

- 缺线程元数据。
- 缺状态、sched、callstack 或 waker。
- waker 歧义。
- 达到深度限制、检测到环或遇到 `udk-irq`。
- 微信首帧目标不存在。

这些情况通过节点的 classification、uncertainty 和 termination reason 表达。场景目标不存在时返回一个 `target_not_found` 终止节点和空 edges，避免成功但无原因的空输出。

## 14. 测试设计

### 14.1 Fact 合同测试

使用小型合成 dataset 分别验证：

- 状态窗口过滤。
- `args/data_dict` 中 `iowait` 和 `caller` 的解码。
- wakeup 目标线程和时间窗口过滤。
- sched slice 查询。
- callstack 重叠查询。
- 微信首帧锚点选择。

### 14.2 Compute 单元测试

使用 fake FactProvider，不加载 dataset，覆盖：

- Running 加 sched/callstack 证据。
- Runnable 调度等待。
- `S/D -> Runnable -> waker` 递归。
- `D-IO`、`D-NIO`、`iowait` 和 blocked caller。
- IO 线程集合、`hmfs_txn` 排除和 `udk-irq` 停止。
- 阻塞上下文下沉且不污染子线程事实。
- 缺状态、缺 waker 和歧义 waker。
- `max_depth` 和环检测。
- 短片段过滤不改变依赖结论。
- sequence/wakeup 边方向。
- 相同 facts 生成稳定 node/edge ID。

### 14.3 Workflow 合同测试

验证：

- discovery 能看到通用 `critical_path`、微信场景 workflow、领域 facts 和 compute。
- 通用 workflow 只组装 provider、compute 请求和 artifacts。
- 微信 workflow 只增加首帧锚点选择。
- `facts/trace_streamer.py` 不再存在。

不为此新增复杂 AST lint；边界通过函数依赖、fake provider 测试和 review 验证。

### 14.4 真实数据端到端验证

使用 `test/test.db`：

1. 物化 dataset。
2. 用明确的 `root_itid/start_ts/end_ts` 运行通用 workflow。
3. 运行微信首帧 workflow。
4. 验证 `path_nodes.parquet` 和 `path_edges.parquet` 可读取。
5. 回查所有 wakeup edges 对应的 `instant` 事实。
6. 抽查 IO、blocked caller、上下文下沉和不确定性字段。
7. 验证没有不存在于 facts 中的依赖边。

## 15. 最小交付切片

实现只修改示例 Pack、相应 Python contract tests 和必要的验证文档：

1. 先为 fact 合同和 fake provider compute 行为补失败测试。
2. 重组 facts 文件并实现规范化查询。
3. 建立 compute models 和逐层遍历算法。
4. 新增通用 workflow，收窄微信 workflow。
5. 更新 discovery、workflow 和真实数据验证。

不借此修改 runtime、datasource 或其他 Pack。

## 16. 方案取舍

未采用“有界事实快照 + 单次纯计算”，因为它会提前读取整个窗口，不能体现逐层按需查询。

未采用“workflow 自己逐层查询”，因为循环、frontier 和领域状态会泄漏到 workflow，使 compute 退化为零散辅助函数。

未采用“拆分递归 SQL”，因为它只是在文件层面移动大 SQL，仍然混合事实访问、图遍历和分类，难以用 fake facts 验证完整策略。

本设计采用显式 FactProvider：fact 保留事实来源知识，compute 保留算法和数据结构，workflow 只负责调度两者。
