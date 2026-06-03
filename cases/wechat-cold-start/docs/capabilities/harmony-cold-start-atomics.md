# 鸿蒙冷启动分析 8 个 Atomic 能力

本文定义用于鸿蒙 App 冷启动关键路径和小核运行时间分析的 8 个原子能力。每个 atomic 只负责一个可复核的问题，不在 atomic 内做跨问题推理；推理和报告由 composite 完成。

适配对象:

- Trace 类型: HarmonyOS htrace/bytrace，经 `kat-rs` 解析后进入 DataFusion 查询环境。
- 核心表: `process`, `thread`, `raw_event`, `thread_state`, `sched_slice`, `callstack`。
- 时间单位: SQL 输入输出默认 ns，报告展示时统一换算为 ms。
- 进程规则: 除 `touchEventDispatch` 外，`HandleLaunchApplication`, `HandleLaunchAbility`, `HandleAbilityTransaction`, `OnVsyncEvent now` 必须区分进程，并优先选择目标 App 进程或有明确目标窗口/Ability 关联证据的进程。

## 1. harmony_process_candidates

```yaml
name: harmony_process_candidates
version: "1.0"
type: atomic
category: harmony_startup
tier: A
```

目的: 根据包名、进程名或关键词定位目标进程，并找出主线程候选。

输入:

- `target_package`: 目标包名或关键词，例如 `com.tencent.wechat`。
- `target_process`: 可选，目标进程名，例如 `.tencent.wechat`。
- `start_hint_ts`: 可选，启动窗口开始时间。
- `end_hint_ts`: 可选，启动窗口结束时间。

输出字段:

| 字段 | 含义 |
| --- | --- |
| `upid` | kat-rs 内部进程 id |
| `pid` | 系统 pid |
| `process_name` | 进程名 |
| `start_ts` / `end_ts` | 进程生命周期 |
| `main_utid` / `main_tid` | 主线程候选 |
| `main_thread_name` | 主线程名 |
| `match_reason` | 命中原因 |
| `confidence` | `high/medium/low` |

核心 SQL:

```sql
SELECT
  p.upid,
  p.pid,
  p.name AS process_name,
  p.start_ts,
  p.end_ts,
  t.utid AS main_utid,
  t.tid AS main_tid,
  t.name AS main_thread_name,
  CASE
    WHEN lower(coalesce(p.name, '')) = lower('${target_process}') THEN 'exact_process'
    WHEN lower(coalesce(p.name, '')) LIKE lower('%${target_package}%') THEN 'package_like'
    WHEN lower(coalesce(t.name, '')) LIKE lower('%${target_package}%') THEN 'thread_like'
    ELSE 'weak_keyword'
  END AS match_reason,
  CASE
    WHEN lower(coalesce(p.name, '')) = lower('${target_process}') THEN 'high'
    WHEN lower(coalesce(p.name, '')) LIKE lower('%${target_package}%') THEN 'medium'
    ELSE 'low'
  END AS confidence
FROM process p
LEFT JOIN thread t
  ON t.upid = p.upid
 AND t.is_main = true
WHERE lower(coalesce(p.name, '')) = lower('${target_process}')
   OR lower(coalesce(p.name, '')) LIKE lower('%${target_package}%')
   OR lower(coalesce(t.name, '')) LIKE lower('%${target_package}%')
ORDER BY
  CASE
    WHEN p.start_ts IS NULL THEN 1
    ELSE 0
  END,
  p.start_ts,
  p.pid;
```

判定规则:

- 如果多个进程同名或同包名，优先选择生命周期覆盖冷启动窗口的主进程。
- 多进程 App 不在本 atomic 内合并，输出候选给后续 tag 归属能力继续判断。

## 2. harmony_cold_start_tag_by_process

```yaml
name: harmony_cold_start_tag_by_process
version: "1.0"
type: atomic
category: harmony_startup
tier: A
```

目的: 搜索鸿蒙冷启动 tag，并把 tag 映射到线程和进程。该 atomic 只列候选，不做最终锚点选择。

输入:

- `target_upids_csv`: 目标进程 `upid` 列表，例如 `329` 或 `329,330`。
- `target_package`: 目标包名或关键词。
- `start_ts`: 可选搜索窗口开始。
- `end_ts`: 可选搜索窗口结束。

输出字段:

| 字段 | 含义 |
| --- | --- |
| `tag_order` | tag 顺序，1 到 5 |
| `tag_name` | 归一化 tag 名 |
| `ts` | 事件时间 |
| `cpu` | 事件所在 CPU |
| `tid` / `utid` | 线程 id |
| `upid` / `pid` | 进程 id |
| `process_name` / `thread_name` | 归属信息 |
| `is_main` | 是否主线程 |
| `process_role` | `input_side/target_process/non_target` |
| `event_name` / `payload_json` | 原始证据 |

核心 SQL:

```sql
WITH tag_events AS (
  SELECT
    ts,
    cpu,
    tid,
    event_name,
    payload_json,
    CASE
      WHEN event_name LIKE '%touchEventDispatch%'
        OR coalesce(payload_json, '') LIKE '%touchEventDispatch%'
      THEN 'touchEventDispatch'
      WHEN event_name LIKE '%HandleLaunchApplication%'
        OR coalesce(payload_json, '') LIKE '%HandleLaunchApplication%'
      THEN 'HandleLaunchApplication'
      WHEN event_name LIKE '%HandleLaunchAbility%'
        OR coalesce(payload_json, '') LIKE '%HandleLaunchAbility%'
      THEN 'HandleLaunchAbility'
      WHEN event_name LIKE '%HandleAbilityTransaction%'
        OR coalesce(payload_json, '') LIKE '%HandleAbilityTransaction%'
      THEN 'HandleAbilityTransaction'
      WHEN event_name LIKE '%OnVsyncEvent now%'
        OR coalesce(payload_json, '') LIKE '%OnVsyncEvent now%'
      THEN 'OnVsyncEvent now'
      ELSE 'unknown'
    END AS tag_name,
    CASE
      WHEN event_name LIKE '%touchEventDispatch%'
        OR coalesce(payload_json, '') LIKE '%touchEventDispatch%'
      THEN 1
      WHEN event_name LIKE '%HandleLaunchApplication%'
        OR coalesce(payload_json, '') LIKE '%HandleLaunchApplication%'
      THEN 2
      WHEN event_name LIKE '%HandleLaunchAbility%'
        OR coalesce(payload_json, '') LIKE '%HandleLaunchAbility%'
      THEN 3
      WHEN event_name LIKE '%HandleAbilityTransaction%'
        OR coalesce(payload_json, '') LIKE '%HandleAbilityTransaction%'
      THEN 4
      WHEN event_name LIKE '%OnVsyncEvent now%'
        OR coalesce(payload_json, '') LIKE '%OnVsyncEvent now%'
      THEN 5
      ELSE 99
    END AS tag_order
  FROM raw_event
  WHERE (${start_ts} IS NULL OR ts >= ${start_ts})
    AND (${end_ts} IS NULL OR ts <= ${end_ts})
    AND (
      event_name LIKE '%touchEventDispatch%'
      OR event_name LIKE '%HandleLaunchApplication%'
      OR event_name LIKE '%HandleLaunchAbility%'
      OR event_name LIKE '%HandleAbilityTransaction%'
      OR event_name LIKE '%OnVsyncEvent now%'
      OR coalesce(payload_json, '') LIKE '%touchEventDispatch%'
      OR coalesce(payload_json, '') LIKE '%HandleLaunchApplication%'
      OR coalesce(payload_json, '') LIKE '%HandleLaunchAbility%'
      OR coalesce(payload_json, '') LIKE '%HandleAbilityTransaction%'
      OR coalesce(payload_json, '') LIKE '%OnVsyncEvent now%'
    )
),
tag_with_process AS (
  SELECT
    e.tag_order,
    e.tag_name,
    e.ts,
    e.cpu,
    e.tid,
    t.utid,
    t.upid,
    p.pid,
    p.name AS process_name,
    t.name AS thread_name,
    t.is_main,
    e.event_name,
    e.payload_json,
    CASE
      WHEN e.tag_name = 'touchEventDispatch' THEN 'input_side'
      WHEN t.upid IN (${target_upids_csv}) THEN 'target_process'
      WHEN lower(coalesce(p.name, '')) LIKE lower('%${target_package}%') THEN 'target_process'
      ELSE 'non_target'
    END AS process_role
  FROM tag_events e
  LEFT JOIN thread t ON e.tid = t.tid
  LEFT JOIN process p ON t.upid = p.upid
)
SELECT
  tag_order,
  tag_name,
  ts,
  cpu,
  tid,
  utid,
  upid,
  pid,
  process_name,
  thread_name,
  is_main,
  process_role,
  event_name,
  payload_json
FROM tag_with_process
ORDER BY ts, tag_order;
```

注意:

- 如果 `target_upids_csv` 为空，生成实现时应移除 `IN (${target_upids_csv})` 条件，避免 SQL 非法。
- `touchEventDispatch` 允许来自 input/system 侧；后四个 tag 必须由 composite 按 `process_role=target_process` 优先选择。

## 3. harmony_cold_start_anchor_select

```yaml
name: harmony_cold_start_anchor_select
version: "1.0"
type: atomic
category: harmony_startup
tier: A
```

目的: 从 tag 候选中选择启动链路锚点，输出确定的 `start -> first target vsync` 时间轴。

输入:

- `tag_candidates`: 来自 `harmony_cold_start_tag_by_process` 的结果集。
- `target_upids_csv`: 目标进程列表。
- `start_fallback_event`: 可选 fallback 名称，例如 `IconStart com.tencent.wechat`。
- `start_hint_ts`: 可选启动前后窗口。

输出字段:

| 字段 | 含义 |
| --- | --- |
| `anchor_name` | `touchEventDispatch` 或 fallback/tag 名 |
| `anchor_order` | 锚点顺序 |
| `ts` | 锚点时间 |
| `upid` / `pid` | 锚点归属进程 |
| `process_name` / `thread_name` | 归属信息 |
| `source_type` | `exact_tag/fallback/raw_event` |
| `confidence` | `high/medium/low` |
| `note` | 缺失、fallback、跨进程说明 |

选择规则:

1. 起点优先使用命中目标启动链路前最近的 `touchEventDispatch`。
2. 如果缺少 `touchEventDispatch`，允许 fallback 到明确目标包名的启动 marker，例如 `IconStart <package>`，但 `confidence=medium`，报告必须说明。
3. 后四个 tag 只从目标进程或目标相关进程选择；同名系统进程 tag 只能作为候选证据，不进入主链路。
4. 每个后续锚点必须满足 `ts >= 前一锚点 ts`；如果出现倒序，输出低置信度并交给 composite 停止或人工确认。
5. `OnVsyncEvent now` 默认选择目标进程内第一个事件；如果选择渲染/系统进程事件，必须额外提供目标窗口关联证据。

核心伪 SQL:

```sql
WITH selected AS (
  SELECT
    1 AS anchor_order,
    'touchEventDispatch' AS anchor_name,
    ts,
    upid,
    pid,
    process_name,
    thread_name,
    'exact_tag' AS source_type,
    'high' AS confidence,
    'input side start anchor' AS note
  FROM tag_candidates
  WHERE tag_name = 'touchEventDispatch'
  ORDER BY ts DESC
  LIMIT 1
),
fallback_start AS (
  SELECT
    1 AS anchor_order,
    '${start_fallback_event}' AS anchor_name,
    ts,
    upid,
    pid,
    process_name,
    thread_name,
    'fallback' AS source_type,
    'medium' AS confidence,
    'touchEventDispatch missing, fallback start marker used' AS note
  FROM fallback_candidates
  ORDER BY ts
  LIMIT 1
),
target_tags AS (
  SELECT *
  FROM tag_candidates
  WHERE process_role = 'target_process'
)
SELECT * FROM selected
UNION ALL
SELECT * FROM fallback_start
WHERE NOT EXISTS (SELECT 1 FROM selected)
UNION ALL
SELECT 2, 'HandleLaunchApplication', ts, upid, pid, process_name, thread_name,
       'exact_tag', 'high', 'target process launch application'
FROM target_tags
WHERE tag_name = 'HandleLaunchApplication'
ORDER BY anchor_order, ts
LIMIT 1;
```

实现建议:

- 真实实现应分别选出 5 个 anchor，再做 `UNION ALL`，避免上面伪 SQL 的 `ORDER BY/LIMIT` 只作用于整体结果。
- Fallback 候选可由 `raw_event` 搜索 `IconStart`, `ability start`, 包名等 marker 得到。

## 4. harmony_cold_start_phase_breakdown

```yaml
name: harmony_cold_start_phase_breakdown
version: "1.0"
type: atomic
category: harmony_startup
tier: B
```

目的: 用锚点切分冷启动阶段，输出每段耗时。

输入:

- `t_start`: `touchEventDispatch` 或 fallback 起点。
- `t_launch_application`: `HandleLaunchApplication`。
- `t_launch_ability`: `HandleLaunchAbility`。
- `t_ability_transaction`: `HandleAbilityTransaction`。
- `t_first_vsync`: 目标进程或目标相关首个 `OnVsyncEvent now`。
- `start_anchor_name`: 起点名称，用于区分 exact/fallback。

输出字段:

| 字段 | 含义 |
| --- | --- |
| `phase` | 阶段 id |
| `start_anchor` / `end_anchor` | 阶段边界 |
| `start_ts` / `end_ts` | 阶段时间 |
| `elapsed_ns` / `elapsed_ms` | 阶段耗时 |
| `phase_order` | 阶段顺序 |

核心 SQL:

```sql
WITH phase_span AS (
  SELECT
    1 AS phase_order,
    'A_input_dispatch_to_app' AS phase,
    '${start_anchor_name}' AS start_anchor,
    'HandleLaunchApplication' AS end_anchor,
    ${t_start} AS start_ts,
    ${t_launch_application} AS end_ts
  UNION ALL
  SELECT
    2,
    'B_launch_application_to_ability',
    'HandleLaunchApplication',
    'HandleLaunchAbility',
    ${t_launch_application},
    ${t_launch_ability}
  UNION ALL
  SELECT
    3,
    'C_launch_ability_to_transaction',
    'HandleLaunchAbility',
    'HandleAbilityTransaction',
    ${t_launch_ability},
    ${t_ability_transaction}
  UNION ALL
  SELECT
    4,
    'D_transaction_to_vsync',
    'HandleAbilityTransaction',
    'OnVsyncEvent now',
    ${t_ability_transaction},
    ${t_first_vsync}
  UNION ALL
  SELECT
    99,
    'TOTAL_start_to_vsync',
    '${start_anchor_name}',
    'OnVsyncEvent now',
    ${t_start},
    ${t_first_vsync}
)
SELECT
  phase_order,
  phase,
  start_anchor,
  end_anchor,
  start_ts,
  end_ts,
  end_ts - start_ts AS elapsed_ns,
  (end_ts - start_ts) / 1000000.0 AS elapsed_ms
FROM phase_span
ORDER BY phase_order;
```

判定规则:

- 任何阶段 `elapsed_ns < 0` 都是链路锚点错误，composite 应停止生成结论。
- `TOTAL` 用于报告，不参与逐阶段占比时应单独处理。

## 5. harmony_main_thread_states_by_phase

```yaml
name: harmony_main_thread_states_by_phase
version: "1.0"
type: atomic
category: harmony_startup
tier: A
```

目的: 统计目标主线程在各冷启动阶段的状态分布，区分 running/runnable/sleeping/uninterruptible/io_wait 等。

输入:

- `main_utid`: 目标主线程 `utid`。
- `phase_span`: 来自 `harmony_cold_start_phase_breakdown` 的 A/B/C/D 阶段。

输出字段:

| 字段 | 含义 |
| --- | --- |
| `phase` | 阶段 |
| `state` | 线程状态 |
| `io_wait` | 是否 IO wait |
| `blocked_function` | 阻塞函数 |
| `waker_utid` | 唤醒线程 |
| `duration_ns` / `duration_ms` | 状态重叠时长 |
| `sample_count` | 重叠片段数 |

核心 SQL:

```sql
WITH phase_span AS (
  SELECT 'A_input_dispatch_to_app' AS phase, ${t_start} AS start_ts, ${t_launch_application} AS end_ts
  UNION ALL
  SELECT 'B_launch_application_to_ability', ${t_launch_application}, ${t_launch_ability}
  UNION ALL
  SELECT 'C_launch_ability_to_transaction', ${t_launch_ability}, ${t_ability_transaction}
  UNION ALL
  SELECT 'D_transaction_to_vsync', ${t_ability_transaction}, ${t_first_vsync}
),
overlap AS (
  SELECT
    p.phase,
    st.state,
    st.io_wait,
    st.blocked_function,
    st.waker_utid,
    CASE
      WHEN st.ts > p.start_ts THEN st.ts
      ELSE p.start_ts
    END AS overlap_start,
    CASE
      WHEN st.ts + coalesce(st.dur, p.end_ts - st.ts) < p.end_ts
      THEN st.ts + coalesce(st.dur, p.end_ts - st.ts)
      ELSE p.end_ts
    END AS overlap_end
  FROM phase_span p
  JOIN thread_state st
    ON st.utid = ${main_utid}
   AND st.ts < p.end_ts
   AND st.ts + coalesce(st.dur, p.end_ts - st.ts) > p.start_ts
)
SELECT
  phase,
  state,
  io_wait,
  blocked_function,
  waker_utid,
  SUM(overlap_end - overlap_start) AS duration_ns,
  SUM(overlap_end - overlap_start) / 1000000.0 AS duration_ms,
  COUNT(*) AS sample_count
FROM overlap
WHERE overlap_end > overlap_start
GROUP BY phase, state, io_wait, blocked_function, waker_utid
ORDER BY phase, duration_ns DESC;
```

判定规则:

- `running` 占比高: 进入 CPU/函数热点归因。
- `runnable` 占比高: 进入调度延迟或 CPU 竞争分析。
- `sleeping/uninterruptible/io_wait` 高: 进入阻塞、Binder、IO 或等待链分析。

## 6. harmony_callstack_hotspots_by_phase

```yaml
name: harmony_callstack_hotspots_by_phase
version: "1.0"
type: atomic
category: harmony_startup
tier: A
```

目的: 在关键阶段内找目标主线程或路径线程的长耗时 callstack span。

输入:

- `path_utids_csv`: 关键路径线程 `utid` 列表，保守起步通常只填主线程。
- `path_tids_csv`: 可选，关键路径线程 `tid` 列表；用于兼容 `callstack.callid` 写入系统 tid 的 trace。
- `phase_span`: A/B/C/D 阶段，或任意分析窗口构造出的单行/多行阶段表。
- `min_dur_ms`: 可选，默认 1 ms。
- `limit`: 可选，默认 50。

输出字段:

| 字段 | 含义 |
| --- | --- |
| `phase` | 阶段 |
| `span_name` | callstack span 名称 |
| `cat` | 分类 |
| `ts` | span 起点 |
| `dur_ns` / `dur_ms` | span 时长 |
| `utid` / `tid` | 所在线程，先通过 `callstack.callid` 兼容匹配 `thread.utid` 或 `thread.tid` |
| `depth` / `parent_id` | 栈关系 |
| `trace_tag` / `custom_category` / `custom_args` | 附加信息 |

核心 SQL:

```sql
WITH phase_span AS (
  SELECT 'A_input_dispatch_to_app' AS phase, ${t_start} AS start_ts, ${t_launch_application} AS end_ts
  UNION ALL
  SELECT 'B_launch_application_to_ability', ${t_launch_application}, ${t_launch_ability}
  UNION ALL
  SELECT 'C_launch_ability_to_transaction', ${t_launch_ability}, ${t_ability_transaction}
  UNION ALL
  SELECT 'D_transaction_to_vsync', ${t_ability_transaction}, ${t_first_vsync}
),
overlap AS (
  SELECT
    p.phase,
    cs.id,
    cs.name AS span_name,
    cs.cat,
    cs.ts,
    cs.dur AS dur_ns,
    cs.dur / 1000000.0 AS dur_ms,
    t.utid,
    t.tid,
    cs.depth,
    cs.parent_id,
    cs.trace_tag,
    cs.custom_category,
    cs.custom_args
  FROM phase_span p
  JOIN callstack cs
    ON cs.ts < p.end_ts
   AND cs.ts + coalesce(cs.dur, p.end_ts - cs.ts) > p.start_ts
  JOIN thread t
    ON cs.callid = t.utid OR cs.callid = t.tid
  WHERE (
      t.utid IN (${path_utids_csv})
      OR t.tid IN (${path_tids_csv})
    )
    AND coalesce(cs.dur, 0) >= ${min_dur_ms} * 1000000
)
SELECT
  phase,
  id,
  span_name,
  cat,
  ts,
  dur_ns,
  dur_ms,
  utid,
  tid,
  depth,
  parent_id,
  trace_tag,
  custom_category,
  custom_args
FROM overlap
ORDER BY phase, dur_ns DESC
LIMIT ${limit};
```

注意:

- 在当前 `kat-rs` schema 中，`callstack.callid` 不是函数 id；不同输入格式可能写入 `utid` 或系统 `tid`。实现时应同时兼容 `thread.utid` 和 `thread.tid`。
- 该 atomic 输出热点，不直接判定根因；composite 结合阶段耗时和线程状态做归因。

## 7. harmony_cpu_cluster_mapping

```yaml
name: harmony_cpu_cluster_mapping
version: "1.0"
type: atomic
category: harmony_cpu
tier: B
```

目的: 给 `sched_slice.cpu` 建立 CPU 到小/中/大核的映射。

输入:

- `mapping_mode`: `manual/default/infer`。
- `small_cpus`: 默认 `0,1,2,3`。
- `middle_cpus`: 默认 `4,5,6,7,8,9`。
- `big_cpus`: 默认 `10,11`。

输出字段:

| 字段 | 含义 |
| --- | --- |
| `cpu` | CPU id |
| `cluster` | `small/middle/big/unknown` |
| `source` | `manual/default/inferred_from_freq` |
| `confidence` | `high/medium/low` |
| `note` | 映射说明 |

默认核心 SQL:

```sql
SELECT 0 AS cpu, 'small' AS cluster, 'default' AS source, 'medium' AS confidence, 'CPU0-3 default small cluster' AS note
UNION ALL SELECT 1, 'small', 'default', 'medium', 'CPU0-3 default small cluster'
UNION ALL SELECT 2, 'small', 'default', 'medium', 'CPU0-3 default small cluster'
UNION ALL SELECT 3, 'small', 'default', 'medium', 'CPU0-3 default small cluster'
UNION ALL SELECT 4, 'middle', 'default', 'medium', 'CPU4-9 default middle cluster'
UNION ALL SELECT 5, 'middle', 'default', 'medium', 'CPU4-9 default middle cluster'
UNION ALL SELECT 6, 'middle', 'default', 'medium', 'CPU4-9 default middle cluster'
UNION ALL SELECT 7, 'middle', 'default', 'medium', 'CPU4-9 default middle cluster'
UNION ALL SELECT 8, 'middle', 'default', 'medium', 'CPU4-9 default middle cluster'
UNION ALL SELECT 9, 'middle', 'default', 'medium', 'CPU4-9 default middle cluster'
UNION ALL SELECT 10, 'big', 'default', 'medium', 'CPU10-11 default big cluster'
UNION ALL SELECT 11, 'big', 'default', 'medium', 'CPU10-11 default big cluster';
```

判定规则:

- 如果设备拓扑或频率表可用，应使用 `manual` 或 `infer` 覆盖默认映射，并把 `confidence` 提升为 `high`。
- 如果只使用默认映射，正式报告必须说明不确定性。

## 8. harmony_critical_path_cpu_cluster_time

```yaml
name: harmony_critical_path_cpu_cluster_time
version: "1.0"
type: atomic
category: harmony_startup
tier: A
```

目的: 计算关键路径线程在各阶段实际运行于小/中/大核上的 CPU 时间。

输入:

- `path_utids_csv`: 关键路径线程列表，保守起步通常只填目标主线程。
- `phase_span`: A/B/C/D 阶段。
- `cpu_cluster`: 来自 `harmony_cpu_cluster_mapping` 的映射。

输出字段:

| 字段 | 含义 |
| --- | --- |
| `phase` | 阶段 |
| `cluster` | CPU cluster |
| `running_ns` / `running_ms` | 实际运行时间 |
| `slice_count` | sched slice 数 |
| `min_cpu` / `max_cpu` | 命中过的 CPU 范围 |

核心 SQL:

```sql
WITH phase_span AS (
  SELECT 'A_input_dispatch_to_app' AS phase, ${t_start} AS start_ts, ${t_launch_application} AS end_ts
  UNION ALL
  SELECT 'B_launch_application_to_ability', ${t_launch_application}, ${t_launch_ability}
  UNION ALL
  SELECT 'C_launch_ability_to_transaction', ${t_launch_ability}, ${t_ability_transaction}
  UNION ALL
  SELECT 'D_transaction_to_vsync', ${t_ability_transaction}, ${t_first_vsync}
),
cpu_cluster AS (
  SELECT 0 AS cpu, 'small' AS cluster
  UNION ALL SELECT 1, 'small'
  UNION ALL SELECT 2, 'small'
  UNION ALL SELECT 3, 'small'
  UNION ALL SELECT 4, 'middle'
  UNION ALL SELECT 5, 'middle'
  UNION ALL SELECT 6, 'middle'
  UNION ALL SELECT 7, 'middle'
  UNION ALL SELECT 8, 'middle'
  UNION ALL SELECT 9, 'middle'
  UNION ALL SELECT 10, 'big'
  UNION ALL SELECT 11, 'big'
),
overlap AS (
  SELECT
    p.phase,
    coalesce(c.cluster, 'unknown') AS cluster,
    s.cpu,
    CASE
      WHEN s.ts > p.start_ts THEN s.ts
      ELSE p.start_ts
    END AS overlap_start,
    CASE
      WHEN s.ts + coalesce(s.dur, p.end_ts - s.ts) < p.end_ts
      THEN s.ts + coalesce(s.dur, p.end_ts - s.ts)
      ELSE p.end_ts
    END AS overlap_end
  FROM phase_span p
  JOIN sched_slice s
    ON s.utid IN (${path_utids_csv})
   AND s.ts < p.end_ts
   AND s.ts + coalesce(s.dur, p.end_ts - s.ts) > p.start_ts
  LEFT JOIN cpu_cluster c ON c.cpu = s.cpu
)
SELECT
  phase,
  cluster,
  SUM(overlap_end - overlap_start) AS running_ns,
  SUM(overlap_end - overlap_start) / 1000000.0 AS running_ms,
  COUNT(*) AS slice_count,
  MIN(cpu) AS min_cpu,
  MAX(cpu) AS max_cpu
FROM overlap
WHERE overlap_end > overlap_start
GROUP BY phase, cluster
ORDER BY phase, cluster;
```

判定规则:

- 小核归因只看 `sched_slice` 的真实 running 时间，不用 `thread_state.running` 代替。
- 如果 `small_ms / total_running_ms` 很低，不应把冷启动慢归因为小核运行。
- 如果 `small_ms` 或 `small_ratio` 高，再结合 callstack 和 runnable 状态判断是调度策略问题、亲和性问题还是业务主动绑核。

## 9. harmony_process_critical_path_in_range

```yaml
name: harmony_process_critical_path_in_range
version: "1.0"
type: atomic
category: harmony_startup
tier: A
```

目的: 查询任意时间段内目标进程的关键路径候选时间线。这个 atomic 不直接宣称“唯一根因”，而是把目标进程线程在该时间段内的长 callstack、线程状态和真实 CPU running 片段统一拉出来，供 composite 进一步判断关键路径。

适用场景:

- 冷启动 A/B/C/D 任意阶段内继续下钻。
- 点击响应、卡顿、白屏、首帧前窗口等非固定 tag 场景。
- 已知一个慢区间，只想问“这个进程在这段时间到底卡在哪里、跑在哪里、等在哪里”。

输入:

- `target_upid`: 目标进程 `upid`，推荐必填。
- `target_pid`: 可选，目标系统 pid；当 `target_upid` 缺失时使用。
- `target_process`: 可选，目标进程名关键词；当 id 缺失时使用。
- `start_ts`: 查询窗口开始时间，必填。
- `end_ts`: 查询窗口结束时间，必填。
- `thread_scope`: `main/all`，默认 `all`。
- `min_span_ms`: callstack 和状态片段最小时长，默认 1 ms。
- `limit`: 输出行数，默认 100。

输出字段:

| 字段 | 含义 |
| --- | --- |
| `path_rank` | 按重叠耗时排序的候选排名 |
| `source` | `callstack/thread_state/sched_slice` |
| `path_kind` | `running_span/runnable_wait/blocking_wait/sleeping/cpu_running` |
| `ts` / `end_ts` | 与查询窗口裁剪后的时间范围 |
| `dur_ns` / `dur_ms` | 裁剪后的持续时间 |
| `upid` / `pid` / `process_name` | 目标进程归属 |
| `utid` / `tid` / `thread_name` / `is_main` | 线程归属 |
| `span_name` | callstack span 名称 |
| `state` / `io_wait` / `blocked_function` / `waker_utid` | 等待与唤醒证据 |
| `cpu` | running slice 所在 CPU |
| `depth` / `parent_id` | callstack 栈关系 |
| `reason` | 候选原因说明 |

核心 SQL:

```sql
WITH target_threads AS (
  SELECT
    p.upid,
    p.pid,
    p.name AS process_name,
    t.utid,
    t.tid,
    t.name AS thread_name,
    t.is_main
  FROM process p
  JOIN thread t ON t.upid = p.upid
  WHERE (
      p.upid = ${target_upid}
      OR p.pid = ${target_pid}
      OR lower(coalesce(p.name, '')) LIKE lower('%${target_process}%')
    )
    AND (
      '${thread_scope}' = 'all'
      OR t.is_main = true
    )
),
callstack_overlap AS (
  SELECT
    'callstack' AS source,
    'running_span' AS path_kind,
    CASE WHEN cs.ts > ${start_ts} THEN cs.ts ELSE ${start_ts} END AS ts,
    CASE
      WHEN cs.ts + coalesce(cs.dur, ${end_ts} - cs.ts) < ${end_ts}
      THEN cs.ts + coalesce(cs.dur, ${end_ts} - cs.ts)
      ELSE ${end_ts}
    END AS end_ts,
    tt.upid,
    tt.pid,
    tt.process_name,
    tt.utid,
    tt.tid,
    tt.thread_name,
    tt.is_main,
    cs.name AS span_name,
    CAST(NULL AS VARCHAR) AS state,
    CAST(NULL AS BOOLEAN) AS io_wait,
    CAST(NULL AS VARCHAR) AS blocked_function,
    CAST(NULL AS BIGINT) AS waker_utid,
    CAST(NULL AS BIGINT) AS cpu,
    cs.depth,
    cs.parent_id,
    'long callstack span overlaps target range' AS reason
  FROM callstack cs
  JOIN target_threads tt ON cs.callid = tt.utid OR cs.callid = tt.tid
  WHERE cs.ts < ${end_ts}
    AND cs.ts + coalesce(cs.dur, ${end_ts} - cs.ts) > ${start_ts}
    AND coalesce(cs.dur, 0) >= ${min_span_ms} * 1000000
),
state_overlap AS (
  SELECT
    'thread_state' AS source,
    CASE
      WHEN st.state = 'running' THEN 'running_state'
      WHEN st.state = 'runnable' THEN 'runnable_wait'
      WHEN st.io_wait = true THEN 'io_wait'
      WHEN st.state = 'uninterruptible' THEN 'blocking_wait'
      ELSE 'sleeping'
    END AS path_kind,
    CASE WHEN st.ts > ${start_ts} THEN st.ts ELSE ${start_ts} END AS ts,
    CASE
      WHEN st.ts + coalesce(st.dur, ${end_ts} - st.ts) < ${end_ts}
      THEN st.ts + coalesce(st.dur, ${end_ts} - st.ts)
      ELSE ${end_ts}
    END AS end_ts,
    tt.upid,
    tt.pid,
    tt.process_name,
    tt.utid,
    tt.tid,
    tt.thread_name,
    tt.is_main,
    CAST(NULL AS VARCHAR) AS span_name,
    st.state,
    st.io_wait,
    st.blocked_function,
    CAST(st.waker_utid AS BIGINT) AS waker_utid,
    CAST(NULL AS BIGINT) AS cpu,
    CAST(NULL AS BIGINT) AS depth,
    CAST(NULL AS BIGINT) AS parent_id,
    'thread state overlaps target range' AS reason
  FROM thread_state st
  JOIN target_threads tt ON st.utid = tt.utid
  WHERE st.ts < ${end_ts}
    AND st.ts + coalesce(st.dur, ${end_ts} - st.ts) > ${start_ts}
    AND coalesce(st.dur, 0) >= ${min_span_ms} * 1000000
),
sched_overlap AS (
  SELECT
    'sched_slice' AS source,
    'cpu_running' AS path_kind,
    CASE WHEN s.ts > ${start_ts} THEN s.ts ELSE ${start_ts} END AS ts,
    CASE
      WHEN s.ts + coalesce(s.dur, ${end_ts} - s.ts) < ${end_ts}
      THEN s.ts + coalesce(s.dur, ${end_ts} - s.ts)
      ELSE ${end_ts}
    END AS end_ts,
    tt.upid,
    tt.pid,
    tt.process_name,
    tt.utid,
    tt.tid,
    tt.thread_name,
    tt.is_main,
    CAST(NULL AS VARCHAR) AS span_name,
    CAST(NULL AS VARCHAR) AS state,
    CAST(NULL AS BOOLEAN) AS io_wait,
    CAST(NULL AS VARCHAR) AS blocked_function,
    CAST(NULL AS BIGINT) AS waker_utid,
    CAST(s.cpu AS BIGINT) AS cpu,
    CAST(NULL AS BIGINT) AS depth,
    CAST(NULL AS BIGINT) AS parent_id,
    'actual CPU running slice overlaps target range' AS reason
  FROM sched_slice s
  JOIN target_threads tt ON s.utid = tt.utid
  WHERE s.ts < ${end_ts}
    AND s.ts + coalesce(s.dur, ${end_ts} - s.ts) > ${start_ts}
    AND coalesce(s.dur, 0) >= ${min_span_ms} * 1000000
),
merged AS (
  SELECT * FROM callstack_overlap
  UNION ALL
  SELECT * FROM state_overlap
  UNION ALL
  SELECT * FROM sched_overlap
),
ranked AS (
  SELECT
    ROW_NUMBER() OVER (ORDER BY end_ts - ts DESC, ts ASC) AS path_rank,
    source,
    path_kind,
    ts,
    end_ts,
    end_ts - ts AS dur_ns,
    (end_ts - ts) / 1000000.0 AS dur_ms,
    upid,
    pid,
    process_name,
    utid,
    tid,
    thread_name,
    is_main,
    span_name,
    state,
    io_wait,
    blocked_function,
    waker_utid,
    cpu,
    depth,
    parent_id,
    reason
  FROM merged
  WHERE end_ts > ts
)
SELECT *
FROM ranked
ORDER BY path_rank
LIMIT ${limit};
```

判定规则:

- 如果 `target_upid/target_pid/target_process` 某项为空，生成实现时应移除对应条件，避免空占位符造成 SQL 非法。
- 这个 atomic 输出的是“关键路径候选”，不是最终根因。
- 如果最大候选主要来自 `callstack/running_span`，composite 应沿 `span_name/depth/parent_id` 还原函数链。
- 如果最大候选主要来自 `thread_state/runnable_wait`，composite 应进入调度延迟和 CPU 竞争分析。
- 如果最大候选主要来自 `thread_state/blocking_wait/io_wait/sleeping`，composite 应沿 `blocked_function/waker_utid` 追等待链。
- 如果要计算小核时间，应把该 atomic 输出的 `utid` 候选传给 `harmony_critical_path_cpu_cluster_time`，而不是只看目标进程总 CPU 时间。
