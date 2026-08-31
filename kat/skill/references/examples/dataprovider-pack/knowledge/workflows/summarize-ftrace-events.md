# Summarize Ftrace Events 分析策略

结果按 `event` 给出计数，是一次低成本的 trace 内容盘点。先识别调度、CPU idle、频率、
中断和目标子系统事件是否存在，再比较数量级；事件计数只能说明“记录了多少次”，不能
直接等同于持续时长或性能影响。

若调度事件异常密集，下一步按 `pid/tgid/cpu` 查看原始 `events`，并结合时间窗口计算切换
间隔；若 idle/frequency 事件突出，继续检查 CPU 维度和相邻时间；若预期事件缺失，先看
`capture` 的 tracer、entries-written 与 entries-in-buffer，判断采集配置或丢失，而不是
立即断言系统没有发生该行为。

跨 trace 或与其他来源联合分析前，核对 `clock_domain`。本 Workflow 的汇总结果不携带
事件时序；需要时序证据时应新增或使用返回明细的 Workflow，而不是从计数反推时间线。
