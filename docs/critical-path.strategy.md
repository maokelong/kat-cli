---
strategy_id: critical-path
display_name: 关键路径抽取
kind: sample
required_capabilities:
  - trace.inspect
  - thread.resolve
  - thread.state_segments
  - sched.slices
  - wakeup.edges
  - callstack.slices
control_flow:
  - sequence
  - branch
  - loop
---

# 专家策略: 关键路径抽取

本文描述专家总结的 trace 关键路径抽取经验。它面向人类维护，不直接执行 SQL；LLM 编排层负责把本文翻译成可执行步骤。

## 1. 目标

关键路径分析要回答:

```text
目标线程为什么没有继续推进？
它是在自己执行、等 CPU、等 IO、等锁，还是等另一个线程唤醒？
```

关键路径不是最长耗时片段，而是决定终点事件何时发生的执行/等待依赖链。

## 2. 输入

- `trace`: 已加载到 DataFusion 的 trace。
- `root_thread`: 根线程，通常是 UI、RS、RT、渲染线程或用户选中的 thread_state task。
- `start_ts`: 分析窗口开始时间。
- `end_ts`: 分析窗口结束时间。
- `max_depth`: 递归追踪最大深度，默认 8。
- `min_segment_ms`: 最小片段时长，默认 0.1 ms。

## 3. 核心经验

### 3.1 从根线程开始，而不是从全局最长片段开始

根线程代表用户关心的进度，例如一帧是否绘出、冷启动是否到首帧、点击是否响应。

如果根线程在等待 worker，那么 worker 才可能进入关键路径。反过来，一个很长的后台线程，如果没有阻塞根线程，就不属于关键路径。

### 3.2 先看状态，再看函数

优先判断线程状态:

| 状态 | 专家解释 |
| --- | --- |
| `Running` | 当前线程在 CPU 上执行，下一步看 `sched_slice` 和 `callstack`。 |
| `Runnable` | 当前线程想跑但没拿到 CPU，下一步看调度延迟、CPU 竞争、优先级、绑核。 |
| `Sleeping` | 当前线程在等事件，下一步看是否有 `waker_utid`。 |
| `Uninterruptible/D` | 内核阻塞、锁、驱动或 IO 候选，下一步看 `blocked_function`。 |
| `D-IO/io_wait` | IO wait 候选，下一步看 IO 线程、block_rq 或存储栈。 |

长 callstack 只能说明函数 span 覆盖窗口，不能单独证明关键路径。

### 3.3 在 Runnable 处追唤醒关系

当线程进入 `Runnable`，说明它刚刚被唤醒或变为可运行。此时查看该时间点附近的唤醒来源:

```text
wakeup_map[wakeup_ts] = waker_utid
```

如果存在 `waker_utid`:

1. 记录当前线程从等待到 Runnable 的边。
2. 把等待区间作为依赖窗口。
3. 递归分析唤醒方线程。

如果不存在 `waker_utid`:

1. 标记为调度等待或等待链不闭合。
2. 不凭空推断依赖线程。

### 3.4 使用 depth 表达依赖层级

根线程为 `depth=0`。

每进入一层唤醒方线程:

```text
depth = depth + 1
```

关系语义:

- `prev/next`: 同一 depth 上的时间顺序。
- `upper/lower`: 不同 depth 的依赖关系。

### 3.5 特殊处理 udk-irq 和 IO 线程

`udk-irq` 不继续递归，避免进入中断线程导致路径发散。但它可以作为 IO Block 的证据。

IO 线程集合可作为初始规则:

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

排除集合:

```text
hmfs_txn
```

### 3.6 阻塞信息需要下沉

如果上层线程处于 D 状态，并带有 `blocked_function` 或 `finnal_blocked_caller`，下层依赖线程应继承这个阻塞上下文。

这样报告时能表达:

```text
UI 线程等待在某个阻塞函数，下层线程执行/IO 是被等待链路的一部分。
```

## 4. 专家判断规则

| 规则 | 结论 |
| --- | --- |
| 根线程长时间 `Running`，且 sched/callstack 支撑 | 关键路径主要是自身执行。 |
| 根线程长时间 `Runnable`，无明确 waker | 关键路径主要是调度等待。 |
| 根线程 `Sleeping/D/IO` 后由 worker 唤醒 | 递归进入 worker。 |
| 依赖线程是 IO 线程或被 `udk-irq` 唤醒 | 标记 IO Block。 |
| `blocked_function` 非空且非 IO | 标记 Non-IO Block。 |
| 线程缺失状态或无 waker | 标记不确定，不生成强结论。 |

## 5. 终止条件

递归在以下情况下停止:

- 到达 `start_ts`。
- 当前线程没有覆盖窗口的状态片段。
- 找不到唤醒方线程。
- 唤醒方是 `udk-irq`。
- 超过 `max_depth`。
- 依赖边重复，出现环。

## 6. 报告要求

报告必须区分:

- 事实: 查询到的状态、时间、线程、唤醒边。
- 推断: 基于事实做出的关键路径分类。
- 不确定性: 缺状态、缺 waker、缺 IO/Binder/锁细节、递归深度截断。

禁止把“最长片段”直接写成“根因”。
