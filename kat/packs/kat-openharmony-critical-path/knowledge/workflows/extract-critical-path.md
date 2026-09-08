# 提取有界关键路径

本 Guide 是当前 PACK 版本的分析策略，不是 Output Schema、可执行计划或完整因果模型。
先以成功 Run 的实际 Output inventory 和 columns 为准；只有 Run ID 时先查询
`information_schema`。确认实际存在相应 relation 和 columns 后，才使用以下示例。

## 先看有界片段摘要

先从 `critical_path_segments` 投影路径解释所需的少量字段，不读取整张表：

```sql
SELECT segment_id, parent_segment_id, depth, duration_ns, clock_domain,
       thread_name, thread_state, segment_kind, relation_to_parent,
       termination_reason, uncertainty_reason
FROM output.critical_path_segments
ORDER BY duration_ns DESC, segment_id
LIMIT 20
```

每行是已观察到的一个片段。`relation_to_parent = wakeup` 表示当前片段在精确边界直接
唤醒下游片段，`same_thread` 表示同一线程的较早片段通向较晚片段，`root` 表示根窗口
锚点。片段时间可以重叠，不要把所有 `duration_ns` 相加后直接称为端到端耗时。

`termination_reason` 是本次有界回溯的确定停止边界，不等同于根因。
`uncertainty_reason` 表示线程状态、调度、调用栈或上游覆盖存在缺口；缺少证据不能解释为
对应活动没有发生，也不能把这条有界路径称为完整的全局因果链。

## 按需读取调用栈证据

只有解释某个已选片段确实需要函数活动时，才查询
`critical_path_callstack_evidence`。`segment_id` 必须来自上一条 Query Result；例如选中
`segment_id = 0` 时：

```sql
SELECT segment_id, callstack_id, parent_callstack_id, callstack_depth,
       start_ts, end_ts, duration_ns, function_name, business_category
FROM output.critical_path_callstack_evidence
WHERE segment_id = 0
ORDER BY segment_id, start_ts, callstack_depth, callstack_id
LIMIT 100
```

调用栈行是裁剪到目标片段的来源证据；`parent_callstack_id` 可能指向 Output 之外。
`business_category` 是当前来源适配器的分类事实，不是因果结论。零行不能解释为“没有
函数活动”，应结合对应片段的 `uncertainty_reason` 判断证据是否缺失。

## 形成结论并停止

结论明确区分：

- 已观察事实：Query Result 中的片段、时间、线程、关系、停止边界、不确定性和调用栈行。
- 推断：哪些已观察片段可能解释用户关心的耗时，以及推断依据的具体 `segment_id`。
- 不确定性：Output 声明的覆盖缺口、参数造成的有界停止，以及当前来源无法回答的部分。

有界证据足够时立即停止。只有用户确实要求扩大范围、本次 inspection 允许、来源仍获授权
且新 Run 预计增加证据时，才考虑调整 `max_depth` 或 `min_segment_ms` 后显式重跑。不要以
相同输入重复执行；缺少来源事实时报告限制或请求新的获授权来源，不由 Guide 补造事实。
