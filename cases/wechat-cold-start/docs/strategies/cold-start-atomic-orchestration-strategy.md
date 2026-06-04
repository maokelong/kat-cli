# 鸿蒙冷启动 Atomic 编排分析策略

本文说明在已经具备原子能力后，如何反向组织一套可复用、可审计的鸿蒙 App 冷启动分析策略。

核心原则:

- Atomic 只产出事实，不直接写根因结论。
- Composite 负责串联 atomic、做质量门禁、生成结构化中间结果。
- Strategy 负责决定分析顺序、分支条件、证据标准和最终报告口径。
- 对 `touchEventDispatch` 之外的冷启动 tag，必须按进程区分，不能把系统进程或其他 App 的同名 tag 混入目标 App 链路。

关联能力文档:

- `../capabilities/harmony-cold-start-atomics.md`
- `../capabilities/harmony-cold-start-composites.md`

## 1. 策略目标

这套策略回答四类问题:

1. 冷启动总耗时是多少，使用的是精确输入起点还是 fallback 起点。
2. 冷启动 tag 链路是否完整，目标进程归属是否可靠。
3. 最大耗时阶段在哪里，该阶段的关键路径主要是执行、调度等待、阻塞等待还是 IO wait。
4. 关键路径实际运行在小核上的时间是多少，小核是否构成主要原因。

最终报告必须把结论拆成三层:

- 事实: atomic 输出的字段和值。
- 推断: 基于事实做出的判断。
- 不确定性: tag 缺失、CPU 拓扑未确认、关键路径跨线程未闭合等风险。

## 2. 输入与输出

必填输入:

- `trace`: htrace/bytrace 文件或已加载 dataset。
- `target_package`: 目标 App 包名或关键词。

推荐输入:

- `target_process`: 目标主进程名。
- `start_hint_ts` / `end_hint_ts`: 粗略启动窗口。
- `start_fallback_event`: 当缺少 `touchEventDispatch` 时使用的 fallback marker，例如 `IconStart com.tencent.wechat`。
- `cpu_topology`: 设备 CPU 到 small/middle/big 的映射。

输出产物:

- 目标进程候选表。
- 冷启动 tag 候选表和选定 anchors。
- A/B/C/D 阶段耗时表。
- 最大阶段的任意区间关键路径候选表。
- 主线程状态分布和 callstack 热点。
- 关键路径 CPU cluster 时间。
- 最终中文报告。

## 3. 总体编排

推荐把分析拆成 7 个阶段:

| 阶段 | 目标 | 首选能力 |
| --- | --- | --- |
| S0 范围确认 | 确认 trace、目标 App、可用表、时间范围 | 环境/表检查，非业务 atomic |
| S1 目标进程定位 | 找到目标进程和主线程候选 | `harmony_process_candidates` |
| S2 冷启动链路还原 | 搜索 tag、选择 anchors、切阶段 | `harmony_cold_start_path_reconstruction` |
| S3 Topdown 阶段判断 | 找最大阶段，决定下钻方向 | `harmony_cold_start_phase_breakdown` |
| S4 任意区间关键路径 | 对最大阶段或指定窗口做关键路径候选查询与筛选 | `harmony_process_critical_path_in_range`, `harmony_critical_path_filter_in_range` |
| S5 深度归因 | 根据状态分布选择函数热点、调度、阻塞或 IO 分支 | `harmony_main_thread_states_by_phase`, `harmony_callstack_hotspots_by_phase` |
| S6 小核归因 | 计算关键路径 running 时间在 CPU cluster 的分布 | `harmony_cpu_cluster_mapping`, `harmony_critical_path_cpu_cluster_time` |

## 4. 主流程

### S1. 目标进程定位

运行:

```text
harmony_process_candidates(target_package, target_process, start_hint_ts, end_hint_ts)
```

产出:

- `selected_upid`
- `selected_pid`
- `selected_main_utid`
- `selected_process_name`
- `confidence`

质量门禁:

- 如果没有目标进程，停止分析并要求补充包名、进程名或时间范围。
- 如果命中多个目标进程，先按生命周期覆盖启动窗口排序，再按主进程名优先。
- 多进程 App 可以保留多个 `upid`，但后续每个 tag 必须显示 `upid/pid/process_name/thread_name`。

### S2. 冷启动链路还原

运行:

```text
harmony_cold_start_path_reconstruction(
  target_package,
  target_process,
  start_hint_ts,
  end_hint_ts,
  start_fallback_event
)
```

内部顺序:

1. `harmony_process_candidates`
2. `harmony_cold_start_tag_by_process`
3. `harmony_cold_start_anchor_select`
4. `harmony_cold_start_phase_breakdown`

tag 选择规则:

- `touchEventDispatch`: 允许来自输入/系统侧。
- `HandleLaunchApplication`: 必须选择目标 App 进程或明确目标相关进程。
- `HandleLaunchAbility`: 必须选择目标 App 进程或明确目标相关进程。
- `HandleAbilityTransaction`: 必须选择目标 App 进程或明确目标相关进程。
- `OnVsyncEvent now`: 默认选择目标 App 进程内首个事件；若选择系统/渲染进程事件，必须提供目标窗口关联证据。

fallback 规则:

- 如果缺少 `touchEventDispatch`，允许使用 `IconStart <package>` 等明确启动 marker。
- 使用 fallback 后，总窗口必须标记为 `fallback window`，不能写成严格输入到首帧耗时。

质量门禁:

- 后四个 tag 任一缺失时，只输出不完整链路，不进入完整冷启动结论。
- 任一阶段 `end_ts < start_ts` 时，视为 anchor 选择错误，停止后续分析。
- 同名 tag 在非目标进程出现时必须列出，但不能默认进入主链路。

### S3. Topdown 阶段判断

运行:

```text
harmony_cold_start_phase_breakdown(anchors)
```

判断:

- 找 `elapsed_ms` 最大的 A/B/C/D 阶段。
- 如果最大阶段占总窗口超过 40%，优先对该阶段做关键路径下钻。
- 如果没有单一阶段超过 40%，按阶段耗时从高到低依次下钻，避免只盯一个点。

阶段含义:

| 阶段 | 范围 | 优先关注 |
| --- | --- | --- |
| A | `touchEventDispatch/IconStart -> HandleLaunchApplication` | 输入分发、系统调度、进程创建 |
| B | `HandleLaunchApplication -> HandleLaunchAbility` | Application 初始化、主线程等待 |
| C | `HandleLaunchAbility -> HandleAbilityTransaction` | Ability 初始化、JS/模块加载、同步等待 |
| D | `HandleAbilityTransaction -> OnVsyncEvent now` | OnStart/onCreate、UI 构建、首帧前等待 |

### S4. 任意区间关键路径下钻

对最大阶段或用户指定时间段先运行宽口径候选查询:

```text
harmony_process_critical_path_in_range(
  target_upid,
  start_ts,
  end_ts,
  thread_scope=all,
  min_span_ms=1
)
```

再运行通用关键路径筛选:

```text
harmony_critical_path_filter_in_range(
  target_upid,
  start_ts,
  end_ts,
  seed_scope=main,
  min_span_ms=1
)
```

这两步合起来是策略里的“通用放大镜”。候选查询回答:

- 这段时间目标进程哪些线程有长 span。
- 哪些线程处于 runnable、blocking、io_wait、sleeping。
- 哪些线程真正有 CPU running slice。

筛选 atomic 回答:

- 哪些片段最像关键路径，排序分数和置信度是多少。
- 等待片段是否存在 `waker_utid`，依赖类型是 `sched_wait/waker_edge/io_wait/blocking_wait` 中哪一种。
- 哪些 `utid` 应进入后续小核时间计算。

输出仍不直接等于最终根因。Strategy 需要按下面规则筛选:

- 优先保留与目标主线程相关的长 `callstack`。
- 如果主线程长时间 `sleeping/blocking/io_wait`，沿 `waker_utid/blocked_function` 找等待来源。
- 如果 worker 线程长 span 被主线程同步等待，worker 才进入关键路径线程集合。
- 如果 worker 线程只是并行后台任务，没有阻塞主线程或首帧链路，不纳入关键路径。

### S5. 深度归因分支

先运行主线程状态:

```text
harmony_main_thread_states_by_phase(main_utid, phases)
```

再按状态分支:

| 主导状态 | 判定 | 下一步 |
| --- | --- | --- |
| `running` 高 | 目标线程一直在执行 | 跑 `harmony_callstack_hotspots_by_phase` |
| `runnable` 高 | 线程想跑但没上 CPU | 进入调度延迟、CPU 竞争、优先级分析 |
| `sleeping` 高 | 可能等待事件、锁、Binder 或定时器 | 用任意区间关键路径查 waker/等待链 |
| `uninterruptible` 或 `io_wait` 高 | 可能 IO 或内核阻塞 | 进入 IO wait、文件/存储、内核阻塞分析 |

函数热点分支:

```text
harmony_callstack_hotspots_by_phase(path_utids_csv, phases, min_dur_ms)
```

热点解释规则:

- 只把与最大阶段重叠的长 span 作为主要证据。
- 嵌套 span 同时出现时，优先解释业务可行动的最内层或关键库名，但保留外层框架链路。
- 若热点主要是 `JsRuntime::LoadModule/RunScript/SourceTextModule::Evaluate`，归因为首启同步 JS/模块加载执行，而不是调度问题。

### S6. 小核归因

先确定 CPU cluster:

```text
harmony_cpu_cluster_mapping(mapping_mode, small_cpus, middle_cpus, big_cpus)
```

再计算关键路径 running 时间:

```text
harmony_critical_path_cpu_cluster_time(path_utids_csv, phase_span, cpu_cluster)
```

关键点:

- 小核时间必须来自 `sched_slice` 的真实 running slice。
- 不用目标进程总 CPU 时间替代关键路径 CPU 时间。
- 不用 `thread_state.running` 替代 CPU cluster 时间，因为它没有 CPU id。

判定口径:

| 指标 | 结论 |
| --- | --- |
| `small_ratio < 5%` | 不支持“小核是主因” |
| `5% <= small_ratio < 20%` | 小核可能有贡献，需要结合最大阶段和热点 |
| `small_ratio >= 20%` 且集中在最大阶段 | 支持小核归因，继续查调度策略/绑核/优先级 |

如果 CPU 拓扑不是设备事实而是默认映射，报告必须写明 `mapping confidence=medium/low`。

## 5. 推荐 Composite 组合

冷启动完整分析使用:

```text
harmony_cold_start_full_report
```

内部推荐顺序:

1. `harmony_cold_start_path_reconstruction`
2. `harmony_cold_start_bottleneck_diagnosis`
3. `harmony_process_range_critical_path_analysis`
4. `harmony_cold_start_small_core_attribution`

任意慢区间分析使用:

```text
harmony_process_range_critical_path_analysis
```

典型输入:

```yaml
target_package: com.tencent.wechat
target_process: .tencent.wechat
start_ts: <max_phase_start_ts>
end_ts: <max_phase_end_ts>
thread_scope: all
min_span_ms: 1
```

优化前后批量对比使用:

```text
harmony_cold_start_batch_compare
```

对比指标:

- 总耗时。
- 最大阶段。
- 主线程 running/runnable/sleeping/io_wait。
- Top callstack hotspot。
- 小核 running 时间和占比。
- anchor 置信度。
- CPU cluster 映射置信度。

## 6. 分支决策树

```text
开始
  |
  v
定位目标进程
  |-- 无目标进程 -> 停止，补充目标信息
  v
查冷启动 tag 并选 anchors
  |-- 后四个目标进程 tag 不全 -> 输出不完整链路
  |-- touchEventDispatch 缺失但 fallback 有 -> 标记 fallback window 后继续
  v
切 A/B/C/D 阶段
  |-- 阶段倒序 -> 停止，重选 anchors
  v
选择最大阶段
  v
任意区间关键路径下钻
  |
  +-- running/callstack 主导 -> 函数热点归因
  +-- runnable 主导 -> 调度延迟/CPU 竞争归因
  +-- sleeping/blocking/io_wait 主导 -> 等待链/IO/锁归因
  v
计算关键路径 CPU cluster 时间
  |
  +-- small_ratio 高 -> 小核归因分支
  +-- small_ratio 低 -> 排除小核主因
  v
生成最终报告
```

## 7. 报告模板

最终报告建议固定为以下结构:

```markdown
# <App> 冷启动分析报告

## 结论摘要
- 总窗口: <start_anchor> -> <end_anchor>, <total_ms> ms, confidence=<...>
- 最大阶段: <phase>, <phase_ms> ms, 占比 <...>%
- 主要瓶颈: <running/callstack/runnable/blocking/io_wait>
- 小核结论: small=<small_ms> ms, ratio=<small_ratio>%, <是否支持小核主因>

## 证据 1: 目标进程与 tag 链路
<process table>
<anchor table>

## 证据 2: 阶段耗时
<phase table>

## 证据 3: 关键路径候选
<range critical path table>

## 证据 4: 线程状态与函数热点
<state table>
<hotspot table>

## 证据 5: CPU cluster 时间
<cluster time table>

## 不确定性
- <tag 缺失/fallback>
- <CPU 拓扑置信度>
- <跨线程关键路径是否闭合>

## 下一步建议
- <按最大阶段和热点给建议>
```

## 8. 结论写法约束

允许的写法:

- “当前 trace 证据显示，最大耗时阶段是 C，主线程以 running 为主，热点集中在 JS 模块加载。”
- “关键路径小核运行时间占比很低，因此当前 trace 不支持小核是主因。”
- “由于 `touchEventDispatch` 缺失，本次总耗时使用 fallback 起点，应视为替代窗口。”

不允许的写法:

- 没有 `sched_slice` cluster 证据时说“小核导致冷启动慢”。
- 把非目标进程的 `OnVsyncEvent now` 当作目标 App 首帧终点。
- 只凭目标进程总 CPU 时间推断关键路径。
- 把 atomic 输出中的候选项直接写成唯一根因。

## 9. 复用方式

分析单次冷启动:

```text
S1 -> S2 -> S3 -> S4(max phase) -> S5 -> S6 -> report
```

分析用户指定慢区间:

```text
S1 -> S4(user range) -> S5 -> S6 -> range report
```

分析优化前后对比:

```text
对每个 trace 执行 full_report -> 汇总 total/max_phase/hotspot/small_ratio -> 比较变化
```

这套组织方式的好处是: 冷启动 tag 链路、任意区间关键路径、小核归因彼此解耦。以后新增 Binder、IO、锁、调度延迟等 atomic 时，只需要挂到 S5 的分支里，不需要推翻主流程。
