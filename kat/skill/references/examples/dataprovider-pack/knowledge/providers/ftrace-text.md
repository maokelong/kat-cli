# Ftrace Text Provider 作者知识

`FtraceTextProvider(source, catalog_root, clock_domain)` 展示本地文本 Provider 的完整闭环：
Python 单遍解析输入，在 `dp.write(schema, destination=...)` 写事务中把标量追加到明确的
relation；后台线程同时写 Parquet，事务正常退出后才一次发布整个多表目录。成功后同一个
实例可反复执行 DataFusion SQL；解析或物化失败时不会留下可查询的半成品 catalog。

物化目录包含两张 Parquet 表：

- `capture`：一行采集元数据，列为 `tracer`、`clock_domain`、
  `ticks_per_second`、`entries_in_buffer`、`entries_written`、`cpu_count`。
- `events`：每条事件一行，列为 `event_index`、`clock_domain`、`clock_value`、
  `cpu`、`comm`、`pid`、可空 `tgid`、`flags`、`event`、`details`。

`capture.clock_domain = events.clock_domain` 描述两表来自同一次时钟配置；
`event_index` 只是在当前文件中的稳定顺序。`clock_value` 由 tracefs 十进制秒值精确换算
成 1 GHz tick，不经过浮点数，也不是 Unix epoch。跨来源比较时间前必须先取得共同
clock domain 或明确的转换证据。

典型查询先用 `capture` 判断 tracer、CPU 数和丢失比例，再按 `events.event` 聚合或按
`pid/tgid/cpu` 过滤。`details` 保留事件专用载荷，若要把某类 details 拆成结构化列，
应在自定义 Provider 的解析阶段扩展 Schema，而不是要求框架猜测事件语义。

新增文件解析器时可以复用同一模式：声明多表 Schema、进入唯一的
`dp.write(schema, destination=...)`、通过 `sink[relation].append(...)` 逐行或逐记录提供
数据，再用 `dp.open(tables=...)` 明确注册已经完整发布的关系。`append()` 成功只表示该行
已通过同步校验并被写事务接纳；只有上下文正常退出才表示整个目录可见。
