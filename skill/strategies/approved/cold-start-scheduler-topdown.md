---
id: cold-start-scheduler-topdown
domain: scheduler-kernel
status: approved
allowed_atomics:
  - trace_sanity_check
  - process_startup_candidates
  - main_thread_state_overview
  - sched_latency_overview
  - cpu_pressure_overview
  - blocking_category_overview
  - cpu_contention_summary
  - thread_state_detail_window
  - top_runnable_competitors
review_required: false
---

# 冷启动调度/内核 Topdown 策略

## 目的

判断当前 trace 的冷启动慢是否主要来自调度/内核侧，并区分 runnable latency、CPU contention 和 blocking。

## 阶段逻辑

1. 先确认 trace 可查询、目标进程和启动窗口。
2. 如果主线程 runnable 时间和 runnable latency 明显偏高，进入 CPU contention 分支。
3. 如果主线程 blocked 时间占主导，进入 blocking 分支；需要查看具体片段时，用 `thread_state_detail_window`，不要直接执行 SQL。
4. 如果两类信号都不明显，报告调度/内核不是当前证据下的主要原因。

## 证据规则

- 每个结论必须引用 atomic 输出字段。
- 如果缺少表、字段或窗口参数，报告缺失信号。
- 只把参与最终判断的 atomic 写入 replay plan。
