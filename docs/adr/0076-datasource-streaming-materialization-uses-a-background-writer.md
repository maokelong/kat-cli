---
status: accepted
---

# Datasource write 是唯一流式物化入口

Data Provider Toolkit 只提供一条 Datasource 物化路径：PACK 作者通过 `with dp.write(schema, destination=...) as sink` 取得按 Datasource Schema relation 名索引的追加写入端，并以 `sink[relation_name].append(...)` 逐行提供解析结果。`dp.write()` 始终表示绑定一个多 relation Datasource Schema 的一次性流式写事务，不再接受已经形成的 Table Mapping；Toolkit 不再提供独立 `dp.materialize()`、`Schema.create()`、`Table(schema)` 或可追加的 `Table.append()`。调用线程负责解析、校验和形成有界批次，后台线程独占 Parquet writer 并与下一批解析并发；批次行数、估算字节数和队列容量由 Toolkit 私下管理，不成为 Pack Authoring API 或硬 RSS 承诺。

一个物化只允许创建它的调用线程使用各 relation 写入端，写入端不承诺线程安全；所有有界批次由一个后台写线程按接收顺序消费。首版不支持多个解析生产者，也不为每张 relation 创建独立工作线程。

Relation 写入端直接依据传给 `dp.write()` 的 Datasource Schema 执行严格逻辑 Schema 校验和 Python 值规范化：字段必须精确匹配，整行验证成功后才被接纳，验证失败不改变物化状态且调用方可以继续追加。单次 `append()` 返回只表示该行已同步通过校验并被当前候选物化接纳，不表示已经写入磁盘或独立提交；只有整个 `with` 正常退出才表示物化成功。后台写入失败会使整个写事务失败，并在后续调用或退出上下文时传播给调用线程。

有界批次按每张 relation 的行数或估算未压缩字节数触发，任一达到即交给后台线程；具体阈值和队列容量属于 Toolkit 内部策略，不进入 Pack Authoring API，也不承诺构成进程 RSS 的硬上限。

每张 Datasource Schema relation 在最终目录中对应一个 `<table_name>.parquet` 文件；连续批次写成同一文件的连续 row group，未接纳任何行的 relation 也形成带正确 Schema 的空文件。首版不引入 part 文件、relation 子目录或新的 Catalog 发现规则。

Destination 沿用现有本地物化边界，必须是父目录已存在且自身尚不存在的 `Path`；write transaction 不覆盖、合并、续写或恢复旧目录。物化中的文件只存在于 destination 同父目录的实例私有候选位置，不形成可查询 Catalog；正常退出 `with` 自动排空队列、关闭 writer、校验 footer 并以不覆盖已有目标的方式发布完整 destination，随后调用方通过 `dp.open(root=destination)` 显式建立 Catalog，不另设公共 `finish()`、`flush()`、`close()` 或 `sink.catalog`。解析、编码、写盘、关闭或发布任一环节失败都不发布部分结果，并只清理本实例的候选产物。

Python 标准库的目录 rename 接口会在部分平台覆盖已有目标，Workflow Host 的既有依赖也没有提供跨 Windows/Linux 的原子 no-replace 目录发布；实现因此直接调用 Windows `MoveFileExW` 和 Linux `renameat2(RENAME_NOREPLACE)`，其他平台明确报不支持，而不新增只包装这两个系统调用的运行时依赖。

后台编码、写盘或关闭一旦失败，整个 write transaction 进入不可恢复的失败状态：阻塞中的生产者必须被唤醒，后续追加或上下文退出传播原始失败，当前 sink 不重试、不续写也不能复用。调用方只能在候选目录清理后重新开始一次新的物化。

有界队列直接复用 Python 3.14 `queue.Queue.shutdown()` 的关闭协议：正常结束采用 graceful shutdown 排空已经接纳的批次，正文取消或后台首次失败采用 immediate shutdown 丢弃尚未消费的批次并唤醒阻塞生产者。owner 侧入队、writer ready 与取消 join 仍使用有限等待，以便等待期间的调用线程中断能够保持正文异常优先；后台消费不再用轮询或私有 sentinel 实现终止。

当 `with` 正文与后台线程同时失败时，正文中的解析异常保持为主异常，后台与清理失败作为附注；正文正常结束时，后台失败是主异常。候选目录清理失败始终只作附注，不覆盖导致物化失败的原始原因。

取消采用线程模型能够安全提供的协作式语义：正文异常或中断后不再接纳新批次，但会等待正在执行的 Parquet 写调用返回，再关闭资源、清理候选目录并传播异常。首版不承诺强制终止或限定底层文件 I/O 的退出时间；需要这种隔离时应另行设计进程边界。

这项能力取代自定义 Parser 通过 `Schema.create()`、大量 `Table.append()` 再 eager `dp.write()` 的来源物化路径。`dp.Table` 收窄为已经完成、不可变且 Arrow-backed 的 eager 单表值，只供 Source query、Fusion query、Arrow interop 和受支持的表格 Run Output 使用，不参与 Datasource 构建或落盘。已经形成的 `pyarrow.Table` 通过 `Table.from_arrow()` 零拷贝接纳；只持有完整 Python 行结果的 Source query 可通过 `Table.from_rows(rows, schema=pyarrow_schema)` 一次性完成严格物理类型转换，避免每个 PACK 重复实现 nullability、范围、时间戳与 Decimal 规则。`from_rows()` 仍是 eager 完成态构造，不可追加，也不形成另一条 Datasource 物化路径。Fusion query 的 eager 结果合同与查询结果过大时的内存问题属于独立决定。

Datasource write transaction 是形成 Parquet catalog 的一次性只写过程，不是另一种 Table builder，也不提供 `len()`、列读取、`to_rows()`、`to_arrow()`、查询中间结果或完成后的继续追加。Workflow Runtime 只把已经完成的不可变 Table 作为 Run Output，并在自己的 Run candidate 内将其保存为 Parquet；实现可以复用私有 Parquet writer，但不能把 Datasource write transaction 当作 Workflow 发布接口或把任意裸文件路径当作 Output。

`dp.write()` 只能作为不可重入的一次性上下文管理器使用；relation 写入端只在 active `with` 正文中有效，进入前、退出后或写事务失败后的操作都稳定失败。调用线程把已经规范化的行组成脱离后不再修改的有界批次，后台线程负责将批次编码为 Arrow 并写入 Parquet。

本决定整体取代 ADR-0065 的可追加 Table、`Schema.create()` 和 eager `dp.write()` 合同；Table 不可变、Schema 只声明多 relation 逻辑结构，Datasource 的构建与持久化统一由本决定中的 write transaction 承担。
