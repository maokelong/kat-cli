# 鸿蒙冷启动 Composite 能力

Composite 负责把 atomic 串成一个可复核的分析过程。原则是: atomic 产出事实，composite 做编排、质量门禁、阈值判定和结论组织。

本文给出 6 个建议 composite。它们可以单独执行，也可以作为更大策略的一部分。

## 1. harmony_cold_start_path_reconstruction

```yaml
name: harmony_cold_start_path_reconstruction
version: "1.0"
type: composite
category: harmony_startup
tier: S
```

目标: 还原鸿蒙 App 冷启动 tag 链路，并切出 A/B/C/D 阶段。

输入:

- `target_package`: 目标包名或关键词。
- `target_process`: 可选，目标进程名。
- `start_hint_ts`: 可选。
- `end_hint_ts`: 可选。
- `start_fallback_event`: 可选，例如 `IconStart com.tencent.wechat`。

步骤:

```yaml
steps:
  - id: process_candidates
    type: skill
    skill: harmony_process_candidates
    params:
      target_package: "${target_package}"
      target_process: "${target_process}"
      start_hint_ts: "${start_hint_ts}"
      end_hint_ts: "${end_hint_ts}"
    save_as: processes

  - id: tags_by_process
    type: skill
    skill: harmony_cold_start_tag_by_process
    params:
      target_upids_csv: "${processes.selected_upids_csv}"
      target_package: "${target_package}"
      start_ts: "${start_hint_ts}"
      end_ts: "${end_hint_ts}"
    save_as: tag_candidates

  - id: anchors
    type: skill
    skill: harmony_cold_start_anchor_select
    params:
      tag_candidates: "${tag_candidates}"
      target_upids_csv: "${processes.selected_upids_csv}"
      start_fallback_event: "${start_fallback_event}"
      start_hint_ts: "${start_hint_ts}"
    save_as: anchors

  - id: phases
    type: skill
    skill: harmony_cold_start_phase_breakdown
    params:
      t_start: "${anchors.t_start}"
      t_launch_application: "${anchors.t_launch_application}"
      t_launch_ability: "${anchors.t_launch_ability}"
      t_ability_transaction: "${anchors.t_ability_transaction}"
      t_first_vsync: "${anchors.t_first_vsync}"
      start_anchor_name: "${anchors.start_anchor_name}"
    save_as: phases
```

质量门禁:

- 没有目标进程: 停止，提示先确认包名或进程名。
- 后四个 tag 任一缺失: 输出不完整链路，不进入耗时结论。
- `touchEventDispatch` 缺失但 fallback 存在: 允许继续，但总耗时标记为 fallback window。
- `OnVsyncEvent now` 只在系统进程命中且无目标窗口关联: 不作为目标首帧终点。

输出:

- 目标进程与主线程候选。
- tag 候选表，含进程归属。
- 选定 anchors。
- A/B/C/D/TOTAL 阶段耗时。

## 2. harmony_cold_start_bottleneck_diagnosis

```yaml
name: harmony_cold_start_bottleneck_diagnosis
version: "1.0"
type: composite
category: harmony_startup
tier: S
```

目标: 找出冷启动最大耗时阶段，并判断主要是运行中、调度等待、阻塞还是 IO wait。

输入:

- 继承 `harmony_cold_start_path_reconstruction` 的输入。
- `min_hotspot_ms`: callstack 热点阈值，默认 1 ms。

步骤:

```yaml
steps:
  - id: path
    type: composite
    skill: harmony_cold_start_path_reconstruction
    params:
      target_package: "${target_package}"
      target_process: "${target_process}"
      start_hint_ts: "${start_hint_ts}"
      end_hint_ts: "${end_hint_ts}"
      start_fallback_event: "${start_fallback_event}"
    save_as: path

  - id: main_thread_states
    type: skill
    skill: harmony_main_thread_states_by_phase
    params:
      main_utid: "${path.selected_main_utid}"
      phase_span: "${path.phases}"
    save_as: states

  - id: callstack_hotspots
    type: skill
    skill: harmony_callstack_hotspots_by_phase
    params:
      path_utids_csv: "${path.selected_main_utid}"
      phase_span: "${path.phases}"
      min_dur_ms: "${min_hotspot_ms}"
      limit: 80
    save_as: hotspots
```

诊断逻辑:

- 最大阶段 `elapsed_ms` 占总耗时超过 40%: 标为主瓶颈阶段。
- 该阶段 `running_ms` 占阶段耗时超过 70%: 优先看 `callstack_hotspots`。
- 该阶段 `runnable_ms` 高: 建议补充调度延迟、CPU 竞争、优先级分析。
- 该阶段 `sleeping/uninterruptible/io_wait` 高: 建议补充 Binder、锁、IO wait 或等待链分析。

输出:

- 最大耗时阶段。
- 主线程状态分布。
- 阶段内 callstack 热点。
- 一句可审计诊断: `阶段 -> 状态类型 -> 热点证据`。

## 3. harmony_cold_start_small_core_attribution

```yaml
name: harmony_cold_start_small_core_attribution
version: "1.0"
type: composite
category: harmony_startup
tier: S
```

目标: 计算关键路径实际运行在小核上的时间，并判断小核是否构成主要原因。

输入:

- 继承 `harmony_cold_start_path_reconstruction` 的输入。
- `path_utids_csv`: 可选，默认目标主线程；如果已识别跨线程关键路径，可传多线程。
- `small_cpus`, `middle_cpus`, `big_cpus`: 可选 CPU 拓扑。

步骤:

```yaml
steps:
  - id: path
    type: composite
    skill: harmony_cold_start_path_reconstruction
    params:
      target_package: "${target_package}"
      target_process: "${target_process}"
      start_hint_ts: "${start_hint_ts}"
      end_hint_ts: "${end_hint_ts}"
      start_fallback_event: "${start_fallback_event}"
    save_as: path

  - id: cpu_cluster
    type: skill
    skill: harmony_cpu_cluster_mapping
    params:
      mapping_mode: "${mapping_mode}"
      small_cpus: "${small_cpus}"
      middle_cpus: "${middle_cpus}"
      big_cpus: "${big_cpus}"
    save_as: cpu_cluster

  - id: cluster_time
    type: skill
    skill: harmony_critical_path_cpu_cluster_time
    params:
      path_utids_csv: "${path_utids_csv:-path.selected_main_utid}"
      phase_span: "${path.phases}"
      cpu_cluster: "${cpu_cluster}"
    save_as: cluster_time
```

诊断逻辑:

- `small_ratio < 5%`: 不支持“小核运行是主因”。
- `5% <= small_ratio < 20%`: 小核有贡献，需要结合阶段与热点判断。
- `small_ratio >= 20%` 且小核集中在最大耗时阶段: 支持小核归因，需要进一步查调度、亲和性、优先级或绑核。

输出:

- 按阶段、按 cluster 的 running 时间。
- 总 small/middle/big 时间和占比。
- 小核归因结论与 CPU 映射置信度。

## 4. harmony_cold_start_full_report

```yaml
name: harmony_cold_start_full_report
version: "1.0"
type: composite
category: harmony_startup
tier: S
```

目标: 生成一份完整的冷启动关键路径报告，覆盖路径、阶段、主线程状态、热点和小核时间。

步骤:

```yaml
steps:
  - id: path
    type: composite
    skill: harmony_cold_start_path_reconstruction
    save_as: path

  - id: bottleneck
    type: composite
    skill: harmony_cold_start_bottleneck_diagnosis
    save_as: bottleneck

  - id: small_core
    type: composite
    skill: harmony_cold_start_small_core_attribution
    save_as: small_core

  - id: synthesis
    type: synthesis
    inputs:
      - path
      - bottleneck
      - small_core
```

报告结构:

1. Trace 与目标进程。
2. 冷启动 tag 链路和缺失/fallback 说明。
3. A/B/C/D 阶段耗时和最大阶段。
4. 主线程状态分布。
5. 最大阶段 callstack 热点。
6. CPU cluster 时间和小核占比。
7. 结论: 是否由小核导致，真正瓶颈在哪里，下一步优化建议。

质量门禁:

- 所有结论必须引用上游 atomic 的字段。
- 如果起点使用 fallback，报告标题或摘要必须标注 `fallback window`。
- 如果 CPU cluster 映射不是设备拓扑确认，报告必须保留 `mapping confidence`。

## 5. harmony_cold_start_batch_compare

```yaml
name: harmony_cold_start_batch_compare
version: "1.0"
type: composite
category: harmony_startup
tier: A
```

目标: 对多份 trace 执行同一冷启动分析签名，输出可横向比较的指标。

输入:

- `trace_list`: 多个 trace 或 dataset。
- `target_package`: 目标包名。
- `target_process`: 可选。
- `mapping_mode`: CPU 映射方式。

步骤:

```yaml
steps:
  - id: per_trace_report
    type: iterator
    for_each: "${trace_list}"
    skill: harmony_cold_start_full_report
    params:
      target_package: "${target_package}"
      target_process: "${target_process}"
      mapping_mode: "${mapping_mode}"
    save_as: reports

  - id: compare_metrics
    type: synthesis
    inputs:
      - reports
```

输出指标:

| 指标 | 来源 |
| --- | --- |
| `total_ms` | `harmony_cold_start_phase_breakdown` |
| `max_phase` / `max_phase_ms` | `phase_breakdown` |
| `main_running_ms` | `main_thread_states_by_phase` |
| `main_runnable_ms` | `main_thread_states_by_phase` |
| `top_hotspot` / `top_hotspot_ms` | `callstack_hotspots_by_phase` |
| `small_ms` / `small_ratio` | `critical_path_cpu_cluster_time` |
| `anchor_confidence` | `anchor_select` |
| `cluster_mapping_confidence` | `cpu_cluster_mapping` |

适用场景:

- 优化前后对比。
- 多版本、多设备、多采样 trace 的趋势分析。
- 判断“小核占比是否稳定升高”或“真正瓶颈是否从 JS 初始化迁移到调度等待”。

## 6. harmony_process_range_critical_path_analysis

```yaml
name: harmony_process_range_critical_path_analysis
version: "1.0"
type: composite
category: harmony_startup
tier: A
```

目标: 给定任意时间段和目标进程，快速输出该进程的关键路径候选、主线程/线程状态归因和下一步下钻方向。它不依赖冷启动 tag，可被冷启动阶段分析、点击响应、卡顿窗口复用。

输入:

- `target_package`: 目标包名或关键词。
- `target_process`: 可选，目标进程名。
- `target_upid`: 可选，已知目标 `upid` 时直接传入。
- `start_ts`: 查询窗口开始时间。
- `end_ts`: 查询窗口结束时间。
- `thread_scope`: `main/all`，默认 `all`。
- `min_span_ms`: 默认 1 ms。

步骤:

```yaml
steps:
  - id: process_candidates
    type: skill
    skill: harmony_process_candidates
    params:
      target_package: "${target_package}"
      target_process: "${target_process}"
      start_hint_ts: "${start_ts}"
      end_hint_ts: "${end_ts}"
    save_as: processes

  - id: range_path
    type: skill
    skill: harmony_process_critical_path_in_range
    params:
      target_upid: "${target_upid:-processes.selected_upid}"
      target_process: "${target_process}"
      start_ts: "${start_ts}"
      end_ts: "${end_ts}"
      thread_scope: "${thread_scope}"
      min_span_ms: "${min_span_ms}"
      limit: 120
    save_as: path_candidates

  - id: cpu_cluster
    type: skill
    skill: harmony_cpu_cluster_mapping
    save_as: cpu_cluster

  - id: small_core_time
    type: skill
    skill: harmony_critical_path_cpu_cluster_time
    params:
      path_utids_csv: "${path_candidates.top_utids_csv}"
      phase_span:
        - phase: "analysis_range"
          start_ts: "${start_ts}"
          end_ts: "${end_ts}"
      cpu_cluster: "${cpu_cluster}"
    save_as: cluster_time
```

诊断逻辑:

- `path_candidates` 中最长项是 `running_span`: 优先看 callstack 名称和父子关系。
- 最长项是 `runnable_wait`: 优先查调度延迟、CPU 竞争、优先级和绑核。
- 最长项是 `blocking_wait/io_wait/sleeping`: 优先沿 `blocked_function` 和 `waker_utid` 追等待链。
- `cluster_time.small_ratio` 高: 进入小核归因；否则不要把慢区间直接归因为小核。

输出:

- 任意时间段的关键路径候选表。
- 候选线程列表和是否主线程。
- CPU cluster 时间与小核占比。
- 下一步建议: 函数热点、调度等待、阻塞等待或小核归因。
