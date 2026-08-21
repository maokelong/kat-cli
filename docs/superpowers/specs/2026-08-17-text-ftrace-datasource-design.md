# 文本 ftrace Datasource 设计

> 状态：已被 Issue #217 的 Proto 统一方案取代。本文件保留 Issue #212 首批四类事件的历史决策；当前有效合同以 `2026-08-19-text-ftrace-proto-compatibility-matrix.md` 为准。当前实现不再发布私有 `ftrace_event_*` 表，而是将文本事件直接格式化为 generated `FtraceEvent`，通过与 Hitrace 相同的 Proto-derived relations 发布。

## 背景

Issue [#212](https://github.com/maokelong/kat-cli/issues/212) 要把 `hitrace --text` 产生的文本 `.ftrace` 转换为 KAT Dataset。当前 KAT 已支持 HiProfiler `.htrace`，但文本 `.ftrace`、raw `.sys` 和 record `.sys` 是不同输入格式；本设计只增加独立的文本 ftrace Datasource，不改变 Hitrace format/domain decoder 或既有表合同。

压缩保存的真实设备样本 `trace/kat_hitrace_text.ftrace.gz` 解压后含 9,043 条事件：

| event | rows |
| --- | ---: |
| `sched_switch` | 6,005 |
| `sched_wakeup` | 3,032 |
| `sched_wakeup_new` | 4 |
| `tracing_mark_write` | 2 |

OpenHarmony Trace Streamer 能把该文件导出为含 89 张实体表的 SQLite，但会把 4 条 `sched_wakeup_new` 合并进 `sched_wakeup`，并把 2 条 `tracing_mark_write` 判为 `invalid_data`。Perfetto Trace Processor 对该样本只摄取 6,005 条 `sched_switch`。两者适合作为解析和分析语义参考，不能提供本切片要求的四类忠实来源事实，也不成为 KAT 运行时依赖。

## 要解决的问题

1. 新增文本 `.ftrace` 的显式 Data Import 入口。
2. 完整保留首个真实样本中的四类事件及其来源字段，不合并事件类型。
3. 只为实际至少产生一行的事件创建 Source table，不创建固定空表集合。
4. 显式保存文本事件的时钟域、原始顺序和 emitter 上下文。
5. 区分合法但未支持的事件与已支持事件或文件结构损坏。

## 不做什么

1. 不解析 raw `.sys`、record `.sys`、压缩 trace 或 `.htrace`。
2. 不修改 `.htrace` / `ftrace-plugin` 解码或既有 `sched_switch` 合同，除非共享 CLI/Datasource 注册存在必要耦合。
3. 不支持四类之外的 ftrace event，不生成 400 多种潜在事件表。
4. 不建立 raw/unknown event 大表，不把四类事件放进一张超宽表。
5. 不引入 Trace Streamer、Perfetto、trace-cmd、SQLite或新的外部 parser 运行时依赖。
6. 不生成 `sched_slice`、`thread_state`、进程线程维表、调度延迟或其他分析派生关系。
7. 不从文件名、时间数值或系统环境猜测 trace clock。

## 公开入口

新增：

```text
kat import ftrace \
  --trace <本地UTF-8文本.ftrace> \
  --clock-domain <boottime|monotonic|ftrace_global>
```

与其他 Data Import 一致，用户可以在 `import` 后、Datasource 子命令前传入：

```text
--dataset <Dataset目录>
--overwrite-dataset
```

只有用户显式要求整体替换目标时才使用 `--overwrite-dataset`；本设计不改变其现有破坏性语义。

`--clock-domain` 使用 KAT `ClockDomain` 名称，不直接泄露 `hitrace --trace_clock` 的 `boot`、`mono`、`global` 缩写。第一版只接受当前能够证明每秒十亿 tick 的三个映射：

| CLI value | `clock_type` | `ticks_per_second` | 对应采集 clock |
| --- | --- | ---: | --- |
| `boottime` | `boottime` | 1,000,000,000 | `boot` |
| `monotonic` | `monotonic` | 1,000,000,000 | `mono` |
| `ftrace_global` | `ftrace_global` | 1,000,000,000 | `global` |

`uptime` 和 `perf` 暂不支持：本切片没有足够证据把它们映射为既有 KAT clock type 和固定十亿 tick 频率。

成功的 operation-specific `result`：

```json
{
  "path": "D:\\datasets\\sample-ftrace",
  "unsupported_events": [
    {
      "name": "irq_handler_entry",
      "count": 42,
      "first_line": 128
    }
  ]
}
```

`path` 是最终 Dataset 的 canonical 绝对 Unicode 路径。`unsupported_events` 始终存在，按 `name` 升序排列；同名事件合并计数，`first_line` 是第一次出现的 1-based UTF-8 文本行号。没有未知事件时返回空数组。

## 输入合同

第一版输入必须是未压缩 UTF-8 文本。逐行处理规则：

1. 空行和以 `#` 开头的标准 ftrace header/comment 忽略。
2. 匹配公共事件头、但事件名未注册的完整行是合法未知内容；只统计名称、数量和首次行号。
3. 四类已注册事件必须完整匹配各自 payload；缺字段、重复字段、非法整数、尾随无法解释内容或溢出使整个 Import 失败。
4. 其他非空文本使整个 Import 失败，诊断包含 1-based 行号。
5. UTF-8 解码失败、读文件失败或一行超过实现固定的有界上限使整个 Import 失败。

公共事件头按结构从右侧识别，不能按空白或第一个 `-` 拆分 emitter 名称。线程名可以包含空格、连字符和括号。支持的形态是：

```text
<emitter-name>-<tid> (<tgid-or------>) [<cpu>] <flags> <seconds.fraction>: <event-name>: <payload>
```

`-------` 表示来源没有提供 TGID，映射为 `emitter_process_id = NULL`；不能伪造为 0，因为 PID 0 有真实 idle 语义。`context_flags` 原样保存为非空字符串，不在本切片解释位语义。

## 时间转换

文本时间不用 `f32` / `f64`。解析器按十进制定点执行：

```text
clock_value = integer_seconds * 1_000_000_000
            + fractional_digits 右补零到 9 位
```

接受 0 到 9 位小数。超过 9 位时，只有多余位全部为零才可无损接受；否则失败。整数乘法、加法或最终值溢出 `UInt64` 时失败。结果仍是指定 `clock_domain` 上的 `ClockValue`，不是 Wall-clock timestamp 或 Duration。

Datasource 为显式指定的 domain 发布一行 `clock_domain`；本输入没有跨 domain snapshot 证据，因此不创建 `clock_snapshot`。

## Source tables

所有事件表都包含公共来源列：

| column | Arrow type | nullable | meaning |
| --- | --- | --- | --- |
| `source_line_number` | `UInt64` | no | 1-based 来源文本行号，也是跨表恢复来源顺序的证据 |
| `clock_domain` | `Utf8` | no | CLI 显式指定的具体时钟域 |
| `clock_value` | `UInt64` | no | 该 domain 上的十亿 tick/秒来源读数 |
| `cpu` | `UInt32` | no | 公共头中的事件 CPU |
| `emitter_thread_name` | `Utf8` | no | 写出事件时公共头中的线程名 |
| `emitter_thread_id` | `Int32` | no | 公共头中的线程 ID |
| `emitter_process_id` | `Int32` | yes | 公共头中的 TGID；`-------` 为 NULL |
| `context_flags` | `Utf8` | no | 公共头中的原始 context flags |

表不承诺 Parquet 或 SQL 隐含行序；需要来源顺序时必须显式按 `source_line_number` 排序。

### `ftrace_event_sched_switch`

在公共列后增加：

| column | Arrow type | nullable |
| --- | --- | --- |
| `previous_thread_name` | `Utf8` | no |
| `previous_thread_id` | `Int32` | no |
| `previous_priority` | `Int32` | no |
| `previous_state` | `Utf8` | no |
| `next_thread_name` | `Utf8` | no |
| `next_thread_id` | `Int32` | no |
| `next_priority` | `Int32` | no |

`previous_state` 保留 `R+`、`D`、`I` 等来源文本，不强转为数值。该表是单条文本事件的机械 Source table，不承担既有 Hitrace `sched_switch` 的 `cpu_switch_sequence`、线程连续性、时钟报告或丢失统计合同。

### `ftrace_event_sched_wakeup`

在公共列后增加：

| column | Arrow type | nullable |
| --- | --- | --- |
| `thread_name` | `Utf8` | no |
| `thread_id` | `Int32` | no |
| `priority` | `Int32` | no |
| `target_cpu` | `UInt32` | no |

只保存文本中真实存在的字段，不补 protobuf 或其他平台变体中的 `success`。

### `ftrace_event_sched_wakeup_new`

Schema 与 `ftrace_event_sched_wakeup` 相同，但保持独立表和来源类型身份，不合并到普通 wakeup。

### `ftrace_event_tracing_mark_write`

在公共列后只增加：

| column | Arrow type | nullable |
| --- | --- | --- |
| `content` | `Utf8` | no |

`content` 保存事件名后面的完整非空 marker 内容。第一版不解释 `trace_event_clock_sync`、phase、cookie、async id 或其他 marker 子协议。

## 按需物化与内存边界

每张事件表使用独立的惰性 spool。第一次看到对应事件时才创建 writer；达到固定 batch 行数后写入临时 Parquet spool并释放该批内存。Import 完成后只准备和发布行数大于零的事件表。

```text
文本文件
  -> 有界逐行读取
  -> 公共头 parser
  -> 四类 payload dispatch
  -> 每表惰性、分批临时 spool
  -> 全文件解析成功
  -> 预读所有非空 spool
  -> Dataset 写事务
  -> clock_domain + 非空事件表
```

未知事件只保留有界聚合统计，不保留完整 payload。任一 parse、spool、schema 或写入失败都发生在成功 Dataset 发布之前，不产生部分 Dataset。

## 模块边界

新增文本 format module，建议位于：

```text
kat/platform/datasource/src/formats/ftrace_text/
```

其窄 Interface 接受 reader、显式 clock domain 和 record sink/capture，产出解析报告；它不识别 `.htrace`、不创建 DataFusion context、不拥有 Dataset 目标，也不实现分析派生。

文本 ftrace materialization 使用独立 sink/capture，并复用现有 Dataset writer、Arrow/Parquet batch和受支持的 `clock_domain`领域类型。不要把文本分支加入 `domains/ftrace::packet` 或 `formats/hitrace`；两种来源只在稳定领域值和 Dataset写入基础设施处共享代码。

Datasource type 是封闭且 bundled 的 `ftrace`。一次 `kat import ftrace` 只使用这个 Datasource，不把文本 ftrace伪装成 Hitrace或Deprecated Trace Streamer输入。

## 与现有领域约束的关系

本设计沿用而不改写现有领域模型：

1. ADR-0020：通过 Dataset 写入接口发布 Source tables，不向 format module 暴露物理目录布局。
2. ADR-0021：一次 `kat import ftrace` 只对应一个文本 ftrace Datasource。
3. ADR-0022：`ftrace` 是 KAT 内建且封闭的 Datasource type，不提供运行时插件注册。
4. ADR-0034：文本中的数值是指定 Clock domain 上的 `ClockValue`，不是 Timestamp 或 Duration。
5. ADR-0042：使用既有 `ClockDomain`/`clock_domain` 表达时钟身份，不从输入内容猜测时钟。

ADR-0025 约束的是 Hitrace protobuf root 中未知 payload 的兼容行为。本设计不改变该行为；文本 ftrace 是独立 Datasource，其 `unsupported_events` 只提供当前导入中未支持事件的名称、计数和首行位置，不引入跨 Datasource 的通用 warning 模型。

现有 `CONTEXT.md` 已定义 Datasource、Dataset、Source table、Clock domain 和 Clock value，本切片没有新增需要进入领域词汇表的概念。该设计也没有改变跨模块持久化、所有权或时间语义决策，因此不新增 ADR；可 review 的输入与表合同记录在本轻量 SDD 中。

## 备选方案与取舍

1. 复用 OpenHarmony Trace Streamer：拒绝。它对真实样本固定导出大量表，合并 `sched_wakeup_new`，且 marker 解析失败；引入其二进制或 SQLite 会增加第二套运行时和表合同。
2. 复用 Perfetto Trace Processor：拒绝。它对真实样本只摄取 `sched_switch`，未忠实保存其余三类；引入第二查询引擎与 KAT Dataset不匹配。
3. 单张 raw/超宽表：拒绝。调用方必须反复解释 payload字符串或大量 nullable列，不能形成四类确定 Source table Interface。
4. 一次生成 400 多张潜在表：拒绝。当前只有四类真实事件和用户需求证据，违反最小切片与非空发布原则。
5. 四类独立表、按需物化：采用。它保持来源类型身份、列式查询效率和小接口，同时允许后续事件按真实需求独立扩展。

## 测试与验证

### Parser单元合同

1. 公共头：idle、nullable TGID、线程名含空格/连字符/括号、不同CPU和flags。
2. 时间：整数、1至9位小数、多余零、非零超精度、乘加溢出和非法字符。
3. 四种payload：正常值、边界整数、缺字段、重复字段、非法箭头和尾随内容。
4. 文件结构：空行、header/comment、未知事件聚合、非法非空行、非法UTF-8和超长行。

### Import与CLI合同

1. 单事件fixture只创建对应事件表和 `clock_domain`。
2. 四类混合fixture创建四张事件表，行值、nullable TGID和`source_line_number`正确。
3. 未出现事件不创建空表。
4. 未知事件导入成功，result按名称排序并给出count/first_line。
5. 已支持事件损坏时操作失败且不发布Dataset。
6. raw/record/压缩/`.htrace`输入被拒绝，不误报为合法零行文本。
7. 现有 Hitrace Import合同测试保持不变。

### 真实fixture回归

对 `trace/kat_hitrace_text.ftrace.gz` 解压后的文本使用 `--clock-domain boottime`：

| table | expected rows |
| --- | ---: |
| `ftrace_event_sched_switch` | 6,005 |
| `ftrace_event_sched_wakeup` | 3,032 |
| `ftrace_event_sched_wakeup_new` | 4 |
| `ftrace_event_tracing_mark_write` | 2 |

四张事件表合计 9,043 行；`clock_domain` 一行。该回归不要求与 Trace Streamer 的派生表逐行相同，只用其 `stat` 总量作为外部对照证据。

提交前验证：

```powershell
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python .github/scripts/test_pr_guard.py
git diff --check
```

## 最小交付切片

一个可独立 review 的 PR 完成：

1. `ftrace` Datasource和CLI注册。
2. 严格公共头、时间和四类payload parser。
3. `clock_domain`及四张按需事件Source tables。
4. `unsupported_events`成功结果与失败诊断。
5. 单元、Import、CLI和真实fixture回归。
6. KAT命令参考更新。

不拆出没有独立用户价值的 parser-only PR，也不顺带扩展其他事件或分析表。
