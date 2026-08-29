# 文本 ftrace 头部校验与提取设计

## 问题与边界

`ftrace2parquet` 当前忽略全部 `#` 行，既不确认输入是完整的 Hitrace 文本 ftrace，也不保存判断采集规模与完整性所需的头部事实。本切片只增加头部语义校验和一行头部输出，不扩展事件类型，不解释 ASCII 图例的固定排版，也不改变显式 `--clock-domain` 合同。

## 头部合同

输入必须以连续的 ftrace 头部开始，之后才允许事件记录。头部必须且只能包含：

- `# tracer: <name>`；名称去除两侧空白后非空；
- `# entries-in-buffer/entries-written: <buffered>/<written> #P:<cpus>`；三个值完整解析，CPU 数大于零，`buffered <= written`；
- `irqs-off`、`need-resched`、`hardirq/softirq`、`preempt-depth` 和 `delay` 图例；`migrate-disable` 图例允许存在；
- 一个包含 `TASK-PID`、`CPU#`、`TIMESTAMP` 和 `FUNCTION` 的列标题；`TGID` 可选。

空白、图例箭头列宽和头部空注释行不构成合同。字段重复、事件开始后再次出现头部、缺失必要项、非法或溢出数字均失败，并携带来源行号或缺失字段名称。

## 提取与一致性

新增单行 `text_ftrace_header.parquet`，字段为：

- `tracer: Utf8`
- `entries_in_buffer: UInt64`
- `entries_written: UInt64`
- `cpu_count: UInt32`
- `has_tgid_column: Boolean`

正文每个语法合法的事件行都计入来源事件数，包括尚未注册的事件。EOF 时该数量必须等于 `entries_in_buffer`。公共事件头中的 CPU 必须小于 `cpu_count`。这两项失败均不发布目标目录。`entries_written - entries_in_buffer` 只作为调用方可计算的覆盖信号，不在本切片增加派生字段。

## 实现切片

逐行读取状态分为 Header 与 Events。Header parser 只提取上述稳定语义，不依赖 ASCII 艺术的固定位置；看到列标题后完成头部，后续空行和注释可忽略，非注释行进入既有事件解析。`OutputTables` 在成功完成全文一致性校验后写入头部行，并与事件表一起原子发布。

## 验证

- 真实 `hitrace --text` 输出成功并保留五个头部字段。
- 覆盖 TGID 列存在和缺席。
- 覆盖必要项缺失、重复、乱序、数值非法/溢出、零 CPU、buffer 关系错误、正文事件数不符、CPU 越界。
- 失败不发布输出；既有事件、批次边界和 CLI 合同保持通过。
