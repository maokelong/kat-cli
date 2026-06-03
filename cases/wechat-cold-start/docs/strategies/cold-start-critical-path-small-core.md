# 鸿蒙 App 冷启动关键路径与小核运行时间分析策略

本文用于分析 HarmonyOS htrace 中的 App 冷启动问题。策略以鸿蒙冷启 tag 点为主时间轴，再结合 `thread_state`、`sched_slice`、`process`、`thread` 还原关键路径，并计算关键路径实际运行在小核上的时间。

核心 tag 点按启动链路排序：

1. `touchEventDispatch`
2. `HandleLaunchApplication`
3. `HandleLaunchAbility`
4. `HandleAbilityTransaction`
5. `OnVsyncEvent now`

关键口径：`touchEventDispatch` 是输入侧起点，可以来自输入/系统线程；其余四个 tag 必须区分进程，至少记录 `upid/pid/process_name/thread_name`，并优先选择目标 App 进程或明确与目标窗口/Ability 相关的进程中的 tag。不能把其他进程的同名 tag 混入目标 App 冷启链路。

时间单位默认是纳秒，报告中统一换算成毫秒：`ms = ns / 1000000.0`。

## 目标

这份策略回答四个问题：

1. 冷启动总耗时是多少，耗时主要落在哪个鸿蒙启动阶段。
2. 首帧完成前的关键路径经过哪些线程、等待点和唤醒关系。
3. 关键路径中实际上 CPU 的运行时间是多少。
4. 关键路径运行时间里，小核、中核、大核分别占多少。

## 必要数据表

稳定核心表：

- `raw_event`：查找鸿蒙冷启 tag，关键列 `ts/cpu/tid/event_name/payload_json`。
- `process`：定位目标进程，关键列 `upid/pid/name/start_ts/end_ts`。
- `thread`：定位目标线程，关键列 `utid/tid/upid/name/is_main`。
- `thread_state`：分析运行、睡眠、阻塞、IO wait、唤醒关系，关键列 `utid/ts/dur/state/io_wait/blocked_function/waker_utid`。
- `sched_slice`：统计真实上 CPU 运行时间，关键列 `cpu/utid/ts/dur/priority/end_state`。

可选增强表：

- `callstack`、`args`：如果当前解析器导出，可用于给阶段命名和确认 span 参数。
- `cpu_frequency` 或同类 CPU 频率表：如果存在，可用于自动推断大小核映射。
- `log`、`hisysevent_all_event`：如果存在，可用于校验业务首屏、包名、Ability 名称。

每次分析先用 `inspect` 或 Web UI 表列表确认当前 trace 实际有哪些表和列，不要假设可选表一定存在。

## 1. 建立鸿蒙冷启 tag 时间轴

先在 `raw_event` 中粗查五个冷启 tag。tag 可能出现在 `event_name`，也可能出现在 `payload_json`，因此两个字段都要查。粗查只用于发现候选事件，不直接作为最终锚点。

如果还没有 `<target_upid_list>`，先执行第 3 节定位目标进程，再回到本节做精确筛选。多进程应用可以把主进程、渲染进程、服务进程都列入候选，但每个 tag 必须保留进程归属。

```sql
SELECT
  ts,
  cpu,
  tid,
  event_name,
  payload_json
FROM raw_event
WHERE event_name LIKE '%touchEventDispatch%'
   OR event_name LIKE '%HandleLaunchApplication%'
   OR event_name LIKE '%HandleLaunchAbility%'
   OR event_name LIKE '%HandleAbilityTransaction%'
   OR event_name LIKE '%OnVsyncEvent now%'
   OR coalesce(payload_json, '') LIKE '%touchEventDispatch%'
   OR coalesce(payload_json, '') LIKE '%HandleLaunchApplication%'
   OR coalesce(payload_json, '') LIKE '%HandleLaunchAbility%'
   OR coalesce(payload_json, '') LIKE '%HandleAbilityTransaction%'
   OR coalesce(payload_json, '') LIKE '%OnVsyncEvent now%'
ORDER BY ts;
```

把命中的事件归一成 `tag_name`，并映射到线程和进程。最终选择时，`touchEventDispatch` 可以不限制目标进程；`HandleLaunchApplication`、`HandleLaunchAbility`、`HandleAbilityTransaction`、`OnVsyncEvent now` 必须按进程区分，通常只保留目标 App 主进程或目标相关进程。

```sql
WITH tag_events AS (
  SELECT
    ts,
    cpu,
    tid,
    event_name,
    payload_json,
    CASE
      WHEN event_name LIKE '%touchEventDispatch%' OR coalesce(payload_json, '') LIKE '%touchEventDispatch%' THEN 'touchEventDispatch'
      WHEN event_name LIKE '%HandleLaunchApplication%' OR coalesce(payload_json, '') LIKE '%HandleLaunchApplication%' THEN 'HandleLaunchApplication'
      WHEN event_name LIKE '%HandleLaunchAbility%' OR coalesce(payload_json, '') LIKE '%HandleLaunchAbility%' THEN 'HandleLaunchAbility'
      WHEN event_name LIKE '%HandleAbilityTransaction%' OR coalesce(payload_json, '') LIKE '%HandleAbilityTransaction%' THEN 'HandleAbilityTransaction'
      WHEN event_name LIKE '%OnVsyncEvent now%' OR coalesce(payload_json, '') LIKE '%OnVsyncEvent now%' THEN 'OnVsyncEvent now'
      ELSE 'unknown'
    END AS tag_name,
    CASE
      WHEN event_name LIKE '%touchEventDispatch%' OR coalesce(payload_json, '') LIKE '%touchEventDispatch%' THEN 1
      WHEN event_name LIKE '%HandleLaunchApplication%' OR coalesce(payload_json, '') LIKE '%HandleLaunchApplication%' THEN 2
      WHEN event_name LIKE '%HandleLaunchAbility%' OR coalesce(payload_json, '') LIKE '%HandleLaunchAbility%' THEN 3
      WHEN event_name LIKE '%HandleAbilityTransaction%' OR coalesce(payload_json, '') LIKE '%HandleAbilityTransaction%' THEN 4
      WHEN event_name LIKE '%OnVsyncEvent now%' OR coalesce(payload_json, '') LIKE '%OnVsyncEvent now%' THEN 5
      ELSE 99
    END AS tag_order
  FROM raw_event
  WHERE event_name LIKE '%touchEventDispatch%'
     OR event_name LIKE '%HandleLaunchApplication%'
     OR event_name LIKE '%HandleLaunchAbility%'
     OR event_name LIKE '%HandleAbilityTransaction%'
     OR event_name LIKE '%OnVsyncEvent now%'
     OR coalesce(payload_json, '') LIKE '%touchEventDispatch%'
     OR coalesce(payload_json, '') LIKE '%HandleLaunchApplication%'
     OR coalesce(payload_json, '') LIKE '%HandleLaunchAbility%'
     OR coalesce(payload_json, '') LIKE '%HandleAbilityTransaction%'
     OR coalesce(payload_json, '') LIKE '%OnVsyncEvent now%'
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
    e.payload_json
  FROM tag_events e
  LEFT JOIN thread t ON e.tid = t.tid
  LEFT JOIN process p ON t.upid = p.upid
)
SELECT
  tag_order,
  tag_name,
  ts,
  tid,
  utid,
  upid,
  pid,
  process_name,
  thread_name,
  is_main,
  cpu,
  event_name,
  payload_json
FROM tag_with_process
WHERE tag_name = 'touchEventDispatch'
   OR upid IN (<target_upid_list>)
   OR lower(coalesce(process_name, '')) LIKE '%<target_package>%'
ORDER BY ts;
```

选择标准：

- `touchEventDispatch`：作为冷启起点，选择触发目标 App 启动的那一次输入分发。若连续点击或 trace 中有多个输入事件，选择目标启动链路前最近的一次。
- `HandleLaunchApplication`：表示应用进程侧开始处理 launch application，必须选择目标 App 进程中的事件。若多个进程都有该 tag，记录各自进程，只把目标启动进程纳入主链路。
- `HandleLaunchAbility`：表示 Ability launch 处理开始，必须选择目标 Ability 所属进程中的事件。多进程应用要区分主进程、渲染进程、服务进程。
- `HandleAbilityTransaction`：表示 Ability transaction 处理，必须选择目标 Ability 所属进程中的事件，不能使用系统进程或其他 App 的同名 tag。
- `OnVsyncEvent now`：作为首帧链路的默认终点候选，必须记录进程归属。若该 tag 出现在渲染/RS/系统进程，需要说明它与目标窗口或目标 Ability 的关联证据；若无法关联，只能作为候选，不能直接作为目标 App 首帧终点。

最终手工确认五个锚点：

```text
t_touch_dispatch      = <touchEventDispatch ts>, tid=<tid>, process=<input/system or unknown>
t_launch_application  = <HandleLaunchApplication ts>, upid=<upid>, pid=<pid>, process=<target_process>
t_launch_ability      = <HandleLaunchAbility ts>, upid=<upid>, pid=<pid>, process=<target_process>
t_ability_transaction = <HandleAbilityTransaction ts>, upid=<upid>, pid=<pid>, process=<target_process>
t_first_vsync         = <OnVsyncEvent now ts>, upid=<upid>, pid=<pid>, process=<target_or_related_process>
```

## 2. 按 tag 切分冷启阶段

以五个 tag 构造四个主阶段：

```text
阶段 A input_dispatch_to_app:
  touchEventDispatch -> HandleLaunchApplication

阶段 B launch_application_to_ability:
  HandleLaunchApplication -> HandleLaunchAbility

阶段 C launch_ability_to_transaction:
  HandleLaunchAbility -> HandleAbilityTransaction

阶段 D transaction_to_vsync:
  HandleAbilityTransaction -> OnVsyncEvent now

总窗口 cold_start:
  touchEventDispatch -> OnVsyncEvent now
```

阶段耗时模板：

```sql
WITH anchors AS (
  SELECT 'touchEventDispatch' AS tag, <t_touch_dispatch> AS ts UNION ALL
  SELECT 'HandleLaunchApplication' AS tag, <t_launch_application> AS ts UNION ALL
  SELECT 'HandleLaunchAbility' AS tag, <t_launch_ability> AS ts UNION ALL
  SELECT 'HandleAbilityTransaction' AS tag, <t_ability_transaction> AS ts UNION ALL
  SELECT 'OnVsyncEvent now' AS tag, <t_first_vsync> AS ts
),
phase_span AS (
  SELECT 'A_input_dispatch_to_app' AS phase, <t_touch_dispatch> AS start_ts, <t_launch_application> AS end_ts UNION ALL
  SELECT 'B_launch_application_to_ability' AS phase, <t_launch_application> AS start_ts, <t_launch_ability> AS end_ts UNION ALL
  SELECT 'C_launch_ability_to_transaction' AS phase, <t_launch_ability> AS start_ts, <t_ability_transaction> AS end_ts UNION ALL
  SELECT 'D_transaction_to_vsync' AS phase, <t_ability_transaction> AS start_ts, <t_first_vsync> AS end_ts
)
SELECT
  phase,
  start_ts,
  end_ts,
  (end_ts - start_ts) / 1000000.0 AS elapsed_ms
FROM phase_span
ORDER BY start_ts;
```

判定口径：

- 阶段 A 高：重点看输入分发、系统调度、目标进程创建、AMS/Ability 管理链路。
- 阶段 B 高：重点看应用进程启动、Application 初始化、主线程 runnable/blocked。
- 阶段 C 高：重点看 Ability launch、页面创建前置、跨线程同步、资源或服务等待。
- 阶段 D 高：重点看 Ability transaction 到首帧/vsync 的 UI 创建、渲染、Vsync、主线程与渲染线程协同。

## 3. 定位目标进程和 tag 所在线程

先用包名定位目标进程：

```sql
SELECT
  upid,
  pid,
  name,
  start_ts,
  end_ts
FROM process
WHERE lower(coalesce(name, '')) LIKE '%<target_package>%'
ORDER BY start_ts NULLS LAST, pid;
```

列出目标进程线程：

```sql
SELECT
  t.utid,
  t.tid,
  t.name,
  t.is_main
FROM thread t
WHERE t.upid = <target_upid>
ORDER BY t.is_main DESC, t.tid;
```

校验最终选定 tag 的线程和进程归属。重点是后四个 tag：它们必须能落到目标 App 进程或目标相关进程；只有 `touchEventDispatch` 可以来自输入/系统侧。

```sql
WITH tag_events AS (
  SELECT
    ts,
    tid,
    event_name,
    payload_json,
    CASE
      WHEN event_name LIKE '%touchEventDispatch%' OR coalesce(payload_json, '') LIKE '%touchEventDispatch%' THEN 'touchEventDispatch'
      WHEN event_name LIKE '%HandleLaunchApplication%' OR coalesce(payload_json, '') LIKE '%HandleLaunchApplication%' THEN 'HandleLaunchApplication'
      WHEN event_name LIKE '%HandleLaunchAbility%' OR coalesce(payload_json, '') LIKE '%HandleLaunchAbility%' THEN 'HandleLaunchAbility'
      WHEN event_name LIKE '%HandleAbilityTransaction%' OR coalesce(payload_json, '') LIKE '%HandleAbilityTransaction%' THEN 'HandleAbilityTransaction'
      WHEN event_name LIKE '%OnVsyncEvent now%' OR coalesce(payload_json, '') LIKE '%OnVsyncEvent now%' THEN 'OnVsyncEvent now'
      ELSE 'unknown'
    END AS tag_name
  FROM raw_event
  WHERE ts >= <t_touch_dispatch>
    AND ts <= <t_first_vsync>
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
)
SELECT
  e.ts,
  e.tag_name,
  e.tid,
  t.utid,
  t.upid,
  p.pid,
  p.name AS process_name,
  t.name AS thread_name,
  t.is_main,
  e.event_name,
  e.payload_json
FROM tag_events e
LEFT JOIN thread t ON e.tid = t.tid
LEFT JOIN process p ON t.upid = p.upid
WHERE e.tag_name = 'touchEventDispatch'
   OR t.upid IN (<target_upid_list>)
   OR lower(coalesce(p.name, '')) LIKE '%<target_package>%'
ORDER BY e.ts;
```

判定口径：

- 目标 App 主线程优先使用 `thread.is_main = true`。
- 如果 `is_main` 不可信，使用 `thread.tid = process.pid`。
- `touchEventDispatch` 不按目标进程过滤，但要和后续目标启动链路在时间上连续。
- `HandleLaunchApplication`、`HandleLaunchAbility`、`HandleAbilityTransaction` 必须属于目标 App 进程或明确的目标相关进程。
- `OnVsyncEvent now` 必须说明进程归属；如果它属于渲染/RS/系统进程，必须给出目标窗口或目标 Ability 关联证据。
- 如果同一个 tag 在多个进程命中，先按 `upid/pid/process_name` 分组列出，再选择目标链路上的那一个。

## 4. 建立关键路径

关键路径不是所有目标进程线程的 CPU 时间总和，而是决定 `OnVsyncEvent now` 到达时间的依赖链。分析方向建议从终点往前追：

1. 以 `OnVsyncEvent now` 或更明确的首帧 marker 为终点。
2. 在阶段 D 中找主线程、渲染/Vsync 相关线程是否等待或被唤醒。
3. 从 `thread_state.waker_utid` 回溯唤醒方；如果没有 waker，使用时间邻近、线程名、blocked_function、tag 事件作为弱证据。
4. 回溯到 `HandleAbilityTransaction`、`HandleLaunchAbility`、`HandleLaunchApplication`，判断每个阶段是谁阻塞了下一阶段。
5. 回溯到 `touchEventDispatch`，确认输入事件到应用启动之间是否存在系统侧或调度侧延迟。
6. 排除不阻塞下一 tag 的并行后台任务。

先看目标主线程在总窗口内的状态：

```sql
SELECT
  ts,
  dur,
  state,
  io_wait,
  blocked_function,
  waker_utid
FROM thread_state
WHERE utid = <main_utid>
  AND ts < <t_first_vsync>
  AND ts + coalesce(dur, 0) > <t_touch_dispatch>
ORDER BY ts;
```

再按 tag 阶段汇总主线程状态：

```sql
WITH phase_span AS (
  SELECT 'A_input_dispatch_to_app' AS phase, <t_touch_dispatch> AS start_ts, <t_launch_application> AS end_ts UNION ALL
  SELECT 'B_launch_application_to_ability' AS phase, <t_launch_application> AS start_ts, <t_launch_ability> AS end_ts UNION ALL
  SELECT 'C_launch_ability_to_transaction' AS phase, <t_launch_ability> AS start_ts, <t_ability_transaction> AS end_ts UNION ALL
  SELECT 'D_transaction_to_vsync' AS phase, <t_ability_transaction> AS start_ts, <t_first_vsync> AS end_ts
),
overlap AS (
  SELECT
    p.phase,
    ts.state,
    ts.io_wait,
    ts.blocked_function,
    ts.waker_utid,
    CASE WHEN ts.ts > p.start_ts THEN ts.ts ELSE p.start_ts END AS clip_start,
    CASE
      WHEN ts.ts + coalesce(ts.dur, 0) < p.end_ts THEN ts.ts + coalesce(ts.dur, 0)
      ELSE p.end_ts
    END AS clip_end
  FROM phase_span p
  JOIN thread_state ts
    ON ts.utid = <main_utid>
   AND ts.ts < p.end_ts
   AND ts.ts + coalesce(ts.dur, 0) > p.start_ts
)
SELECT
  phase,
  state,
  io_wait,
  blocked_function,
  waker_utid,
  sum(clip_end - clip_start) / 1000000.0 AS duration_ms
FROM overlap
WHERE clip_end > clip_start
GROUP BY phase, state, io_wait, blocked_function, waker_utid
ORDER BY phase, duration_ms DESC;
```

查看目标进程所有线程在各阶段的 CPU 运行量，找出可能参与关键路径的 worker：

```sql
WITH phase_span AS (
  SELECT 'A_input_dispatch_to_app' AS phase, <t_touch_dispatch> AS start_ts, <t_launch_application> AS end_ts UNION ALL
  SELECT 'B_launch_application_to_ability' AS phase, <t_launch_application> AS start_ts, <t_launch_ability> AS end_ts UNION ALL
  SELECT 'C_launch_ability_to_transaction' AS phase, <t_launch_ability> AS start_ts, <t_ability_transaction> AS end_ts UNION ALL
  SELECT 'D_transaction_to_vsync' AS phase, <t_ability_transaction> AS start_ts, <t_first_vsync> AS end_ts
),
overlap AS (
  SELECT
    p.phase,
    t.utid,
    t.tid,
    t.name,
    t.is_main,
    CASE WHEN s.ts > p.start_ts THEN s.ts ELSE p.start_ts END AS clip_start,
    CASE
      WHEN s.ts + coalesce(s.dur, 0) < p.end_ts THEN s.ts + coalesce(s.dur, 0)
      ELSE p.end_ts
    END AS clip_end
  FROM phase_span p
  JOIN sched_slice s
    ON s.ts < p.end_ts
   AND s.ts + coalesce(s.dur, 0) > p.start_ts
  JOIN thread t ON s.utid = t.utid
  WHERE t.upid = <target_upid>
)
SELECT
  phase,
  utid,
  tid,
  name,
  is_main,
  sum(clip_end - clip_start) / 1000000.0 AS running_ms
FROM overlap
WHERE clip_end > clip_start
GROUP BY phase, utid, tid, name, is_main
ORDER BY phase, running_ms DESC;
```

关键路径整理为 `path_span`，每一行表示“阻塞下一 tag 或首帧推进”的线程片段。多进程场景下必须记录 `upid/pid/process_name`，不能只记录线程名。

```text
seq | phase | start_ts | end_ts | upid | pid | process_name | utid | thread_name | reason | evidence
1   | A     | ...      | ...    | ...  | ... | input/system | ...  | input       | dispatch to launch | touchEventDispatch -> target HandleLaunchApplication
2   | B     | ...      | ...    | ...  | ... | target app   | ...  | app main    | Application init   | target-process HandleLaunchApplication
3   | C     | ...      | ...    | ...  | ... | target app   | ...  | app main    | Ability launch     | target-process HandleLaunchAbility -> HandleAbilityTransaction
4   | D     | ...      | ...    | ...  | ... | render/vsync | ...  | render      | first vsync        | target-related OnVsyncEvent now
```

如果某段只靠时间邻近推断，`evidence` 必须写明“推断”，不要和 `waker_utid` 这类强证据混在一起。

## 5. 推断大小核映射

小核映射必须先确认，不能只凭 CPU 编号猜。

优先方法：

1. 如果当前 trace 有 `cpu_frequency` 或类似频率表，按 CPU 最大频率分组，最低频率簇为小核候选。
2. 如果 trace 没有频率表，用设备 SoC 文档、内核拓扑或平台已知映射。
3. 如果只能经验判断，报告中标注“不确定”。

频率表模板，具体列名以当前 schema 为准：

```sql
SELECT
  cpu,
  max(freq) AS max_freq
FROM cpu_frequency
GROUP BY cpu
ORDER BY cpu;
```

在正式计算时，把确认后的映射写成 `cpu_cluster` CTE。下面只是示例，需要替换成当前设备真实映射：

```sql
WITH cpu_cluster AS (
  SELECT 0 AS cpu, 'small' AS cluster UNION ALL
  SELECT 1 AS cpu, 'small' AS cluster UNION ALL
  SELECT 2 AS cpu, 'small' AS cluster UNION ALL
  SELECT 3 AS cpu, 'small' AS cluster UNION ALL
  SELECT 4 AS cpu, 'middle' AS cluster UNION ALL
  SELECT 5 AS cpu, 'middle' AS cluster UNION ALL
  SELECT 6 AS cpu, 'middle' AS cluster UNION ALL
  SELECT 7 AS cpu, 'middle' AS cluster UNION ALL
  SELECT 8 AS cpu, 'big' AS cluster UNION ALL
  SELECT 9 AS cpu, 'big' AS cluster
)
SELECT * FROM cpu_cluster;
```

## 6. 计算关键路径小核运行时间

核心口径：

- 只统计 `sched_slice`，因为它代表线程实际在 CPU 上运行。
- 只统计关键路径 `path_span` 中的 `utid + 时间段`，不要把整个进程 CPU 时间当成关键路径。
- 对 `sched_slice` 与 `path_span` 做交集裁剪。
- 先输出各 CPU 簇运行时间，再单独看小核占比。
- 等待态本身不贡献 CPU 运行时间；等待时间要用 `thread_state` 另算。

关键路径 CPU 簇分布模板：

```sql
WITH path_span AS (
  SELECT 1 AS seq, 'A_input_dispatch_to_app' AS phase, <utid_1> AS utid, <path_1_start_ts> AS start_ts, <path_1_end_ts> AS end_ts UNION ALL
  SELECT 2 AS seq, 'B_launch_application_to_ability' AS phase, <utid_2> AS utid, <path_2_start_ts> AS start_ts, <path_2_end_ts> AS end_ts UNION ALL
  SELECT 3 AS seq, 'C_launch_ability_to_transaction' AS phase, <utid_3> AS utid, <path_3_start_ts> AS start_ts, <path_3_end_ts> AS end_ts UNION ALL
  SELECT 4 AS seq, 'D_transaction_to_vsync' AS phase, <utid_4> AS utid, <path_4_start_ts> AS start_ts, <path_4_end_ts> AS end_ts
),
cpu_cluster AS (
  SELECT 0 AS cpu, 'small' AS cluster UNION ALL
  SELECT 1 AS cpu, 'small' AS cluster UNION ALL
  SELECT 2 AS cpu, 'small' AS cluster UNION ALL
  SELECT 3 AS cpu, 'small' AS cluster UNION ALL
  SELECT 4 AS cpu, 'middle' AS cluster UNION ALL
  SELECT 5 AS cpu, 'middle' AS cluster UNION ALL
  SELECT 6 AS cpu, 'middle' AS cluster UNION ALL
  SELECT 7 AS cpu, 'middle' AS cluster UNION ALL
  SELECT 8 AS cpu, 'big' AS cluster UNION ALL
  SELECT 9 AS cpu, 'big' AS cluster
),
overlap AS (
  SELECT
    p.seq,
    p.phase,
    s.cpu,
    c.cluster,
    CASE WHEN s.ts > p.start_ts THEN s.ts ELSE p.start_ts END AS clip_start,
    CASE
      WHEN s.ts + coalesce(s.dur, 0) < p.end_ts THEN s.ts + coalesce(s.dur, 0)
      ELSE p.end_ts
    END AS clip_end
  FROM path_span p
  JOIN sched_slice s
    ON s.utid = p.utid
   AND s.ts < p.end_ts
   AND s.ts + coalesce(s.dur, 0) > p.start_ts
  JOIN cpu_cluster c ON s.cpu = c.cpu
)
SELECT
  cluster,
  sum(clip_end - clip_start) / 1000000.0 AS running_ms
FROM overlap
WHERE clip_end > clip_start
GROUP BY cluster
ORDER BY running_ms DESC;
```

按鸿蒙 tag 阶段拆分小核运行时间：

```sql
WITH path_span AS (
  SELECT 1 AS seq, 'A_input_dispatch_to_app' AS phase, <utid_1> AS utid, <path_1_start_ts> AS start_ts, <path_1_end_ts> AS end_ts UNION ALL
  SELECT 2 AS seq, 'B_launch_application_to_ability' AS phase, <utid_2> AS utid, <path_2_start_ts> AS start_ts, <path_2_end_ts> AS end_ts UNION ALL
  SELECT 3 AS seq, 'C_launch_ability_to_transaction' AS phase, <utid_3> AS utid, <path_3_start_ts> AS start_ts, <path_3_end_ts> AS end_ts UNION ALL
  SELECT 4 AS seq, 'D_transaction_to_vsync' AS phase, <utid_4> AS utid, <path_4_start_ts> AS start_ts, <path_4_end_ts> AS end_ts
),
cpu_cluster AS (
  SELECT 0 AS cpu, 'small' AS cluster UNION ALL
  SELECT 1 AS cpu, 'small' AS cluster UNION ALL
  SELECT 2 AS cpu, 'small' AS cluster UNION ALL
  SELECT 3 AS cpu, 'small' AS cluster
),
overlap AS (
  SELECT
    p.seq,
    p.phase,
    CASE WHEN s.ts > p.start_ts THEN s.ts ELSE p.start_ts END AS clip_start,
    CASE
      WHEN s.ts + coalesce(s.dur, 0) < p.end_ts THEN s.ts + coalesce(s.dur, 0)
      ELSE p.end_ts
    END AS clip_end
  FROM path_span p
  JOIN sched_slice s
    ON s.utid = p.utid
   AND s.ts < p.end_ts
   AND s.ts + coalesce(s.dur, 0) > p.start_ts
  JOIN cpu_cluster c ON s.cpu = c.cpu
)
SELECT
  seq,
  phase,
  sum(clip_end - clip_start) / 1000000.0 AS small_core_running_ms
FROM overlap
WHERE clip_end > clip_start
GROUP BY seq, phase
ORDER BY seq;
```

## 7. 区分“小核运行慢”和“等待导致慢”

冷启慢不一定是小核导致。必须同时给出等待时间：

```sql
WITH path_span AS (
  SELECT 1 AS seq, 'A_input_dispatch_to_app' AS phase, <utid_1> AS utid, <path_1_start_ts> AS start_ts, <path_1_end_ts> AS end_ts UNION ALL
  SELECT 2 AS seq, 'B_launch_application_to_ability' AS phase, <utid_2> AS utid, <path_2_start_ts> AS start_ts, <path_2_end_ts> AS end_ts
),
overlap AS (
  SELECT
    p.seq,
    p.phase,
    ts.state,
    ts.io_wait,
    ts.blocked_function,
    ts.waker_utid,
    CASE WHEN ts.ts > p.start_ts THEN ts.ts ELSE p.start_ts END AS clip_start,
    CASE
      WHEN ts.ts + coalesce(ts.dur, 0) < p.end_ts THEN ts.ts + coalesce(ts.dur, 0)
      ELSE p.end_ts
    END AS clip_end
  FROM path_span p
  JOIN thread_state ts
    ON ts.utid = p.utid
   AND ts.ts < p.end_ts
   AND ts.ts + coalesce(ts.dur, 0) > p.start_ts
)
SELECT
  phase,
  state,
  io_wait,
  blocked_function,
  waker_utid,
  sum(clip_end - clip_start) / 1000000.0 AS duration_ms
FROM overlap
WHERE clip_end > clip_start
GROUP BY phase, state, io_wait, blocked_function, waker_utid
ORDER BY phase, duration_ms DESC;
```

解释规则：

- 小核运行时间高、等待低：关键路径确实在小核执行较多，关注调度策略、优先级、绑核、负载迁移。
- 小核运行时间低、等待高：瓶颈不是小核，优先追锁、IO、Binder、Ability 生命周期同步等待、资源加载。
- Runnable 等待高：关注 CPU 竞争、线程优先级、调度延迟、系统侧负载。
- 阶段 D 高但 CPU 时间不高：重点看 UI/渲染/Vsync 依赖和主线程是否错过 vsync。

## 8. 输出报告格式

建议固定输出：

```text
目标 App:
Trace:

鸿蒙冷启 tag:
  touchEventDispatch: ts=, tid=, process=
  HandleLaunchApplication: ts=, upid=, pid=, process=, thread=
  HandleLaunchAbility: ts=, upid=, pid=, process=, thread=
  HandleAbilityTransaction: ts=, upid=, pid=, process=, thread=
  OnVsyncEvent now: ts=, upid=, pid=, process=, thread=, target_window_evidence=

阶段耗时:
  A touchEventDispatch -> HandleLaunchApplication:
  B HandleLaunchApplication -> HandleLaunchAbility:
  C HandleLaunchAbility -> HandleAbilityTransaction:
  D HandleAbilityTransaction -> OnVsyncEvent now:
  total touchEventDispatch -> OnVsyncEvent now:

关键路径:
  1. <phase> <thread> <start-end> <elapsed_ms> <evidence>
  2. ...

关键路径 CPU 运行时间:
  small_core_running_ms:
  middle_core_running_ms:
  big_core_running_ms:
  total_running_ms:

关键路径等待时间:
  blocked_ms:
  io_wait_ms:
  runnable_delay_ms:

判断:
  慢在哪个鸿蒙 tag 阶段:
  是否主要由小核运行导致:
  主要瓶颈:
  证据:
  不确定性:

建议:
  1. ...
  2. ...
```

## 常见误区

- 不要只用进程总 CPU 时间判断关键路径。关键路径必须能解释下一个 tag 或首帧为什么没到。
- 不要把 `OnVsyncEvent now` 机械等同于首帧完成；如果有更明确的窗口可见或 first draw marker，需要一起校验。
- 不要把其他进程的 `HandleLaunchApplication`、`HandleLaunchAbility`、`HandleAbilityTransaction` 或 `OnVsyncEvent now` 当成目标 App 的冷启 tag。
- 不要把小核 CPU 时间等同于冷启动耗时；冷启动耗时是墙上时间，小核时间只是运行时间的一部分。
- 不要在没有确认 CPU cluster 映射时下“小核导致”结论。
- 不要忽略系统侧线程。`touchEventDispatch -> HandleLaunchApplication` 这段经常涉及输入、系统服务、Ability 管理和调度。
- 不要忽略等待。很多冷启慢是主线程等 worker、IO、Binder 或 Ability transaction，而不是 CPU 算得慢。
