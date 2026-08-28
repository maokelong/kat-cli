# 文本 ftrace 转 Parquet 独立程序设计

## 问题

需要一个可直接调用的单用途程序，把未压缩 UTF-8 文本 ftrace 转成一个可被 Arrow/Parquet 工具读取的文件。当前目标不是把该来源集成进 KAT Dataset，也不是建立事件级领域模型。

## 范围

新增工作区二进制 crate `ftrace2parquet`：

```text
ftrace2parquet --input <trace.ftrace> --output <trace.parquet> --clock-domain <name>
```

成功时输出文件存在且进程退出码为 0；失败时退出码非零且不留下目标文件。目标已经存在时失败，不提供覆盖选项。

## Parquet 合同

单个固定 Schema，每个合法事件一行：

| 列 | Arrow 类型 | 可空 | 含义 |
| --- | --- | --- | --- |
| `source_event_sequence` | UInt64 | 否 | 从 0 开始的事件行顺序 |
| `clock_domain` | Utf8 | 否 | 命令行明确提供的时钟域 |
| `clock_value` | UInt64 | 否 | 文本秒值按十进制定点换算为每秒 1,000,000,000 tick |
| `cpu` | UInt32 | 否 | 方括号中的 CPU |
| `emitter_thread_name` | Utf8 | 否 | 从右侧结构解析得到的线程名 |
| `emitter_thread_id` | Int32 | 否 | 线程 ID |
| `emitter_process_id` | Int32 | 是 | TGID；`-------` 表示缺席 |
| `context_flags` | Utf8 | 否 | 文本公共头中的原始标志 |
| `event_name` | Utf8 | 否 | 事件名称 |
| `payload` | Utf8 | 否 | 事件分隔符后的完整内容，包括尾随空格 |

程序不理解 `sched_switch` 等具体 payload，不区分已知与未知事件。这样所有合法事件均被转换，Schema 不随事件集合变化。

## 解析与资源边界

- 使用 `BufRead` 逐行读取，单行最大 1 MiB。
- 忽略空行和左侧去空白后以 `#` 开头的注释。
- 从右侧已知结构解析公共头，允许线程名包含空格、连字符和括号。
- 数字字段必须完整消费并通过目标整数范围校验。
- Clock value 不经过浮点数；超过 9 位的小数只有多余位全为 0 才接受。
- 以 8,192 行为一个 Arrow/Parquet 写入批次，内存不随输入总行数增长。
- 零事件输入失败。

## 发布边界

解析和写入发生在输出同目录的临时文件中。只有全部输入成功、最后批次写入成功且 Parquet writer 正常 close 后才将临时文件重命名为目标。目标存在时在读取输入前失败。失败产生的临时文件由临时文件所有者清理。

## 验证

- CLI help 和必填参数合同。
- 公共头边界：空格、连字符、括号、缺失 TGID、context flags。
- 十进制定点精度、整数溢出、损坏行、非法 UTF-8、超长行和零事件。
- payload 原样保留，未知事件同样产生行。
- 超过 8,192 行的输入产生完整且有序的 Parquet 数据。
- 失败不发布目标，已有目标不会被覆盖。
- 真实文本 ftrace fixture 能转换并由 Parquet reader 读取。

## 非目标

- KAT CLI、Skill、Datasource、Dataset 或 PACK 集成。
- Proto 或按事件类型生成多张关系表。
- 解释任何事件 payload 或 marker 子协议。
- 输入格式自动探测、压缩输入、stdin/stdout、目录输出、追加和覆盖。
- 跨版本 Schema 迁移或兼容承诺。
