---
status: accepted
---

# Hitrace Datasource 拥有可复用 Trace facts

Hitrace Datasource 在导入时拥有直接事件和跨记录规范化的可复用 Trace facts，表名与 Schema 不从 Trace Streamer 推导；PACK 只消费这些事实形成分析，不以隐藏 Workflow 或 Run Output 制造共享输入，Hitrace 也不为无事实的数据创建零行占位表。

只有当前多个真实消费者共同需要的稳定语义才能下沉为事实；单个 PACK 的策略留在 PACK，现阶段 `thread-cpu-time` 直接从 `sched_switch` 计算，不增加 `thread_running_interval`、`sched_slice` 等平行表。
