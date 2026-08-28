# 文本 ftrace 转 Proto 派生 Parquet 独立程序设计

## 问题与边界

需要一个可直接调用的单用途程序，把未压缩 UTF-8 文本 ftrace 转成由 Proto 定义的多张 Parquet 表。它是独立二进制，不是 KAT CLI、Datasource、Dataset、PACK 或查询层。

```text
ftrace2parquet --input <trace.ftrace> --output <directory> --clock-domain <name>
```

输出目录必须不存在。成功时整体发布目录；失败时不留下目标。第一版不覆盖、追加或合并输出。

## Proto 合同

crate 自有 `TextFtraceEvent` 根消息。公共事件头位于根，具体事件通过 oneof 表达。首批支持 `SchedSwitch`、`SchedWakeup`、`SchedWakeupNew` 和 `TracingMarkWrite`。

每条已支持事件产生：

1. `text_ftrace_event_occurrence` 来源实例；
2. `text_ftrace_event` 根；
3. 恰好一个所选 payload 子表行。

关系使用 `_kat_row_id` 和 `_kat_parent_row_id`。来源实例保存 `source_event_sequence`；合法未知事件占用序号但不产生任何关系行。只创建实际含行的 payload Parquet 文件。

## 输出文件

固定根文件：

- `text_ftrace_event_occurrence.parquet`
- `text_ftrace_event.parquet`

按实际事件出现创建：

- `text_ftrace_event_sched_switch.parquet`
- `text_ftrace_event_sched_wakeup.parquet`
- `text_ftrace_event_sched_wakeup_new.parquet`
- `text_ftrace_event_tracing_mark_write.parquet`

公共根字段为 `clock_domain`、`clock_value`、`cpu`、`emitter_thread_name`、`emitter_thread_id`、可空 `emitter_process_id` 和 `context_flags`。payload 表字段与 Proto 消息字段一一对应。

## 解析与资源边界

- `BufRead` 逐行读取，单行最大 1 MiB。
- 忽略空行和左侧去空白后以 `#` 开头的注释。
- 从右侧结构解析公共头，支持线程名空格、连字符、括号和 tracefs 列宽填充。
- Clock value 使用十进制定点转换为每秒 1,000,000,000 tick，不经过浮点数。
- 已注册事件 payload 必须完整匹配字段、数字范围和尾部边界，否则整次失败。
- `tracing_mark_write` 内容不解释并保留尾随空格。
- 每张关系最多缓冲 8,192 行，然后写入 Parquet row group。

## 发布

所有 Parquet 文件先写入输出同级临时目录。只有输入读取、各关系 flush、Parquet close 全部成功后，才把临时目录重命名为目标。零条已支持事件失败。

## 验证

- Proto build 和 CLI 参数合同。
- occurrence → root → payload 关联及未知事件序号间隙。
- 四个 oneof 变体和只发布实际出现的子表。
- 公共头、缺失 TGID、定点精度、溢出、字段损坏、非法 UTF-8、超长行。
- 超过 8,192 条支持事件后行数和关联完整。
- 失败不发布目标、已有目标不覆盖。
- 真实文本 ftrace smoke conversion。

## 非目标

- KAT 集成、Dataset marker、catalog、Operation log 或查询接口。
- 上游 HiProfiler ftrace-plugin Proto。
- 首批四类之外的 payload 类型。
- 保存未知事件 raw payload。
- 压缩输入、stdin/stdout、覆盖、追加或 Schema 配置。
