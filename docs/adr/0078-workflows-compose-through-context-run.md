---
status: accepted
---

# Workflow 通过 Context 显式调用其他 Workflow

KAT 只保留一个 Workflow 概念：Workflow 既可以直接形成表格证据，也可以在普通 Python 控制流中通过同步 `ctx.run()` 调用其他 Workflow，再按需查询子 Run Catalog 并组装自己的输出。调用期间不插入 AI 决策；AI 只在 Workflow 已发布输出之后依据 Guide 解释证据。KAT 不提供 DAG、调度器、异步 Workflow 或专用并行语法，执行顺序、条件和异常处理遵循普通 Python 语义。

每个可执行 Workflow 都必须具有受 inspection 约束的 Python 入口。自由格式 Markdown 只能作为该 Workflow 可选引用的 Guide，不能单独声明、调用或执行一个 Workflow；需要确定性组合多个 Workflow 的分析任务必须用一个普通 Python 入口表达调用和返回，再用 Guide 保留证据解释空间。

Guide 的作用域始终是声明它的 Workflow 所发布的 Run，而不是整个调用树。组合执行全部结束后，KAT Skill 默认先读取父 Guide 与父 Run Output inventory；只有父 Guide 要求汇总子结论或父级证据不足时，才沿 `child_runs` 按需选择相关子 Run，并将每个选中的 Run 与它自己的 Guide 配对解释，随后回到父 Guide 汇总。KAT 不把子 Guide 自动拼接、继承或替代父 Guide，缺省 Guide 的 Run 也不要求生成一份独立解释。所有解释都发生在确定性 Workflow 执行之后，中间的子级解释只存在于模型工作上下文，不成为 Run Output、Manifest 或另一种持久对象。

“相关子 Run”“证据不足”和“最少证据”是 Guide 与模型在解释阶段作出的尽力判断，不是 Runtime 可判定的不变量或新的 Guide 语法。若同一父 Run 的多个直接子 Run 具有相同 PACK 与 Workflow，而公开事实不足以进一步区分，Skill 把所有匹配项视为候选；这种歧义会影响结论时应向用户说明，而不是猜测调用顺序。

`kat inspect session --session <session-id>` 在 Issue #248 已定义的平坦 Run inventory 中，为每个 Run 增加一个按 Run ID 稳定排序的直接 `child_runs` 数组，叶子固定返回空数组；其余字段仍精确为 `run_id`、`pack`、`workflow` 与公开 Output inventory，不递归嵌入子 Run，也不加入 inputs、物理路径、失败调用或执行计划。Skill 选中一个 Run 后，继续使用既有 `kat inspect workflow --session <session-id> --run <run-id>` 取得该 Workflow 的当前 Guide，并用 `kat query` 的同一双 ID 查询证据。不新增 `inspect run` 命令。

Guide 在解释阶段建议 AI 继续调用的 Workflow 不属于已经结束的父 Workflow 执行，也不能事后改写其不可变 Manifest；这些后续调用在同一 Analysis Session 中形成新的独立根 Run。只有父 Workflow 的 Python 入口在执行期间通过 `ctx.run()` 发起的调用才进入该父 Run 的 `child_runs`。若某个调用是父结果成立所必需的确定性组成部分，作者必须把它写入父 Workflow 的普通 Python 控制流，而不是依赖 Guide 追认关系。

KAT Skill 可以为当前用户任务临时依次调用多个已有 Workflow，并在同一 Analysis Session 中查询各自证据、形成 Analysis Result；这段临时调用序列不获得 Workflow 身份，也不创建父 Run。只有需要复用、inspection 和测试的组合才固化为具有 Python 入口的正式 Workflow，KAT 不动态生成临时 PACK 或 Python Workflow。

Analysis Session 在任何生产 Workflow 执行前由独立 `kat session create` 操作显式发布，生产 `kat run` 必须指定一个已经存在的 Session。KAT Skill 为新分析先创建 Session，再调用顶层 Workflow，因此父级失败时仍然持有可用于检查已发布子 Run 的 Session ID；Run failure Response 继续不携带部分 `result`。这取代 ADR-0077 中“省略 Session 时由首个成功 Run 创建 Session”以及“不提供 Session create”的决定。

`kat session create` 不接受 Session ID 参数；成功时以唯一原子提交点发布空且已验证的 Session 目录与标记，并精确返回 `{"session_id":"<session-id>"}`，失败时不返回部分 `result`。`kat inspect session` 对空 Session 返回 `runs: []`。`kat run` 的 `--session` 在生产执行中成为必填参数，不隐式创建、复用或猜测 Session。

`ctx.run()` 必须可以由同一父 Workflow 创建并负责等待的多个 Python 线程安全地并发调用；每次调用仍是当前 Analysis Session 中相互独立的子 Run。KAT 不拥有线程池、并发上限、取消、回滚或完成顺序，父 Workflow 必须在返回前等待自己启动的工作，不能留下未受直接 Host 等待的后台执行。Context 跟踪仍在进行的调用；父函数返回时如果还有活动调用，KAT 停止接受该 Context 的新调用、拒绝发布父 Run 并报告 `kat.RunError`，再等待已经启动的调用全部结束后关闭 Runtime。等待期间成功发布的子 Run 继续留在 Session 中。

Context 在同一把锁下按 `OPEN → CLOSING → CLOSED` 变化。每次 `ctx.run()` 在写协议请求前原子检查 `OPEN` 并登记为活动调用；父入口返回时原子转入 `CLOSING`。登记先发生的调用属于已经启动的调用并必须收拢，关闭先发生的调用同步得到 `kat.RunError` 且不启动子 Runtime；转入关闭时仍有已登记调用，父 Run 必须失败。Host 只跟踪通过 Context 成功登记的调用，等待任意其他用户线程和完成对子 Catalog 的读取仍是 Workflow 作者责任。

线程中的异常仍遵循普通 Python 语义：工作线程里未捕获的 `kat.RunError` 只结束该线程，单独调用 `Thread.join()` 不会把异常抛回父 Workflow 入口。需要让任一子调用失败导致父 Run 失败时，作者应使用 `concurrent.futures` 并在父线程调用每个 `Future.result()`，或显式保存并重抛工作线程异常。只有最终逃出父 Workflow 入口的异常才直接决定父执行失败；无论父级如何处理，所有已经启动的调用仍需收拢，已经发布的子 Run 仍保留。

并发子调用完全继承 Issue #248 的同 Session 写入合同：每个子执行使用独立 candidate ID 隔离 Run candidate 与 scratch，普通执行共同持有 Session shared lease 而不互相串行；竞争同一 datasource basename 的 Provider 分别形成完整候选，再通过既有 no-replace 发布和兼容性校验选择或拒绝胜者。`ctx.run()` 不增加 Session 全局执行锁、物化覆盖或第二套冲突策略。

`kat test` 中的 `kat_run` 使用测试命令范围内的临时 Analysis Session 与工作空间执行组合 Workflow。嵌套调用复用生产环境相同的 PACK discovery、Input Compiler、独立子 Runtime、异常传播和子 Run Catalog 返回路径；被测 PACK 固定使用 `kat test --pack-dir` 指定的精确目录，其他目标只从正常默认或已安装 PACK roots 发现。首版不增加额外 dependency checkout 目录参数；本地 sibling PACK 必须先安装到正常发现范围。所有测试 Session、候选 Run 和已发布 Run 都位于测试临时根目录，不进入生产存储。`kat_run` 保持现有测试合同，始终向测试代码返回按 Output name 索引的 `dict[str, pyarrow.Table]`；Workflow 内部的 `ctx.run()` 才返回 `dp.Catalog`。

PACK 测试继续只通过 `kat_run` 断言业务 Output，不增加读取子 Run ledger 的 fixture API。`child_runs`、发布边界、递归拒绝、线程竞态和测试临时 Session 生命周期由 KAT 自身的 CLI/Runtime 平台一致性测试覆盖；失败测试目录是否保留沿用现有 pytest 策略，本决策不建立新的清理承诺。

每次 `ctx.run()` 都必须显式提供全局 PACK name 与该 PACK 内的 Workflow name，即使调用目标位于当前 PACK；没有隐式当前 PACK、相对名称或导入后直接调用的捷径。KAT 对每次调用重新执行正式 PACK discovery、目标 Workflow inspection、输入校验与独立 Runtime 执行，并自动沿用父 Workflow 的 Analysis Session；同 PACK 与跨 PACK 调用使用完全相同的边界。

嵌套调用只能继承顶层 `kat run` 或 `kat test` 已经确定的 PACK discovery roots，包括顶层命令提供的 `--pack-dir`。`ctx.run()` 不接受 PACK 路径、额外搜索目录或修改 discovery scope 的参数；目标名称在该范围内缺失或存在歧义时，在启动子 Runtime 前抛出 `kat.RunError`。因此跨 PACK 组合不会扩大本次执行的发现边界。

Workflow declaration、装饰器和 PACK 配置不预先列出可能调用的子 Workflow，也不引入 `children` 或依赖图。普通 Python 中实际执行的 `ctx.run(pack_name, workflow_name, /, **inputs)` 是唯一调用定义，目标在每次调用时发现并校验；Workflow inspection 因而不承诺静态枚举潜在子调用。Run Manifest 只保留已经发生的直接调用事实，避免静态声明与条件代码形成两份会漂移的真相。

Workflow declaration 与 inspection 也不新增 Output name 或 Schema 声明。`ctx.run()` 返回的实际 `catalog.tables` 是子 Run Output name 的唯一权威，列名与类型由 DataFusion 在 SQL planning 时从已验证 Parquet footer 取得；缺失 relation、缺列或类型不兼容使父 Workflow 按普通执行错误失败。跨 PACK 的组合关系因而是运行时接口依赖，子 Output 的改名、删列或不兼容改型对消费方属于破坏性变更，并由组合 PACK 的 `kat test` 集成用例发现，而不是再维护一份容易与实际 Output 漂移的静态 Schema。

KAT 在每次调用前检查当前活动 Workflow 调用链；目标 `(PACK, Workflow)` 已经存在于该调用链时立即以 `kat.RunError` 拒绝调用。因此 `A → B → A` 等直接或间接递归不成立，也不提供可配置最大调用深度。已经结束的调用不再属于活动链，所以父级可以顺序重复调用同一 Workflow，兄弟调用也可以调用同一目标；每次成功调用仍发布独立 Run。

`ctx.run(pack_name, workflow_name, /, **inputs)` 的 PACK 与 Workflow name 是两个必填、仅限位置的路由参数；目标 Workflow 的全部输入只接受普通 Python 关键字参数，因此不会保留或占用目标可能声明的 `pack`、`workflow` 输入名。嵌套值必须与目标标注严格对应：`str`、范围在有符号 64 位内的精确 `int`、有限的精确 `float`、精确 `bool`、允许集合内的字符串 `Literal`、`kat.Duration` 实例或 `kat.WallClockTimestamp` 实例；只有 Optional 参数接受 `None`。`"5"` 不转换为整数，整数不转换为浮点数，`bool` 不作为 `int`，也不接受这些内建类型的子类。省略值仍由目标 Workflow Runtime 内已有的唯一 Input Compiler 应用默认值并完成最终校验；缺少、未知或不兼容的参数在目标 Workflow 函数执行前，由调用侧传输准入或目标 Input Compiler 拒绝，并统一表现为 `kat.RunError`。

私有 JSONL RPC 使用带显式类型 tag 的标量编码，避免 JSON number 丢失 int64 边界或混淆 boolean，并在子 Runtime 中重建上述 Python 值后交给同一个 Input Compiler。它不接受 CLI argv、option spelling，也不由父级把值重新编码成命令行字符串。子 Run Manifest 继续采用现有规范化表示，使等价的顶层 CLI 输入与嵌套 Python 输入产生相同 effective inputs：int64 与 Duration 使用规范十进制字符串，Wall-clock timestamp 使用规范文本，其余保持既有 JSON 投影。

`ctx.run()` 返回的 `dp.Catalog` 不成为 Workflow input value，也不能放进后续 `ctx.run()` 的 `inputs`。父 Workflow 可以用现有 `dp.DataFusionProvider(catalog=child)` 按需查询子结果、形成自己的 Table，或从查询结果中确定性提取目标 Input Compiler 已支持的普通值再发起后续调用。KAT 不为嵌套调用增加 CLI 无法表达的 Catalog/Table 参数、Run Output 引用参数、第二套 Manifest 输入编码或额外的跨 Runtime 数据输入协议。

`ctx.run()` 只在子 Run 完整持久发布后返回一个由 KAT 构造的只读 `dp.Catalog`。Catalog 的 relation name 与子 Run Output name 完全一致：子 Workflow 返回单个 Table 时为 `main`，返回字典时保留各个 key。Host 从自己刚发布并重新验证的 Run 中取得 relation name 与规范化 Parquet 路径，经私有 Runtime 协议交给父侧构造 Catalog；这些路径不进入受支持的 Catalog 公共接口、CLI Response 或公共诊断。PACK 是受信任的本地 Python，而不是文件系统沙箱，因此不承诺恶意反射代码绝对无法取得路径。该路径不跨进程传输 Arrow 数据，也不把所有子 Output eagerly 加载到父进程；父级只有把 Catalog 交给 DataFusion Provider 查询时才扫描所需 Parquet 数据。

一个 Run 的唯一发布提交点是 Host 以 no-replace 方式原子提交其最终 Manifest。在此之前，KAT 必须满足既有 Runtime Response、进程退出与 Operation log flush 等发布门槛，并完成 Workflow Output 写入和关闭、目录层级与非 reparse 普通文件校验、Parquet footer/Schema 校验、Host 对 Output metadata 与 `child_runs` 的组装，以及该候选 scratch 的清理；任一步失败都不形成 Run。Manifest 提交后 Run 即已发布且不可变。Host 随后仍从这个已发布 Run 重新解析并验证 Output，再为父 Runtime 构造 Catalog；此时若因外部损坏或基础设施错误失败，`ctx.run()` 失败且读取闭合失败，但不回滚已经发布的 Run。

父 Runtime 与 Rust Host 复用该 Runtime 独占的 stdin/stdout，建立严格、私有的双向 JSONL RPC 控制面，不新增 socket、常驻服务或递归 `kat` CLI。Python 在导入 PACK 前保留协议文件描述符，把 PACK 可见 stdin 指向空输入，并在文件描述符层把 PACK stdout 重定向到 stderr，避免 Python `print()`、原生扩展或子进程输出破坏协议帧。Rust Host 必须持续读取请求、调度子 Runtime 并写回响应，最后才能等待父 Runtime 退出；先等待进程再处理请求会形成确定死锁。

本决策仅在 `run_workflow` 与 `test_pack` 的嵌套调用、测试控制帧范围内，局部取代 ADR-0010、ADR-0057 关于 Runtime 标准流只承载诊断而不参与控制协议的描述，以及 ADR-0053 关于当前唯一生产 worker 的描述。最终 Runtime Response 仍通过原子文件交付，stdin/stdout 只传输上述私有 JSONL 控制帧；直接 Host 继续拥有 Runtime 进程生命周期和回收责任，PACK 可见输出也继续进入既有诊断投影。局部取代的原因是 Host 必须在 Runtime 存活期间服务子调用，才能避免 wait-first 死锁并隔离 PACK stdout。

每次嵌套调用使用父 Runtime 内单调分配的 `call_id`，并发请求可以乱序完成，但由唯一响应 reader 分派回对应调用线程，写入端只在单帧 write/flush 期间串行化。调用成功帧只携带 `call_id` 与按 relation name 稳定排序的非空 Catalog relation 描述，不携带 Session ID、Run ID、`child_runs`、列统计或 Arrow 数据。Host 隐含拥有 Analysis Session、PACK discovery roots、活动调用链和直接子 Run ledger；它不接受 Python 自报的 Run ID、路径或父子关系。

严格完成顺序是：子 Runtime 完成，Host 持久发布子 Manifest，立即把子 Run 加入父级直接子 Run ledger，从已发布 Run 重新解析并验证 Output 文件，写回 Catalog 描述，再由对应父调用线程构造 `dp.Catalog`。响应 reader 只负责路由帧，不读取 Parquet footer。只有父调用线程完成或确认 Catalog 构造失败后，本次 `ctx.run()` 才离开活动集合。

Host 使用规范化 Path 组件而非字符串前缀验证 relation 路径，要求 Manifest、Run 目录、`outputs` 目录和 Parquet 文件都位于预期层级并拒绝符号链接；Windows 还拒绝任意 reparse point、设备名、替代数据流和路径分隔符逃逸。外部同用户进程在验证后的篡改属于不支持的 Run corruption，而不是私有 IPC 能消除的安全边界。低层路径只进入私有日志，公共 `kat.RunError` 不回显它们。

首版不扩展 DataFusion Provider 直接注册或联邦查询多个 Catalog。父 Workflow 调用多个子 Workflow 时，分别用 `dp.DataFusionProvider(catalog=child)` 从每个 Catalog 选择必要列、过滤或聚合为较小的 Table，再用已有 `dp.DataFusionProvider(tables={...})` 融合这些 Table。只有出现必须直接 join 多个大型 Catalog 的真实用例后，才另行设计 relation namespace 与冲突规则。

一次 `ctx.run()` 失败以唯一公开类型 `kat.RunError` 回到父 Workflow；是否捕获、降级或让父 Workflow 失败完全使用普通 Python 异常语义。RunError 文本只供人阅读，程序不能解析；它不公开稳定 phase、Session ID、Run ID、物理路径或 `run_published` 等状态字段。任意 RunError 都不能假定可安全自动重试，因为失败调用可能已经留下成功 Run、成功后代或外部副作用；作者若主动重试，就接受这是可能产生重复 Run 的全新执行。

未捕获异常或父级后续失败时不发布父 Run，但此前已经成功发布的子 Run 保留在当前 Analysis Session 中，不因父级失败而回滚或删除。子 Run 完整发布之后若 Catalog 描述交付、文件能力验证或父级 Catalog 构造失败，子 Run 仍然成功并保留，而 `ctx.run()` 因未能交付可用 Catalog 而失败；若父级捕获该错误后仍成功发布，Host 仍把这个已经发布的直接子 Run 纳入父 Manifest 的 `child_runs`。未捕获 RunError 由顶层 KAT Response 投影安全诊断，底层路径和基础设施细节只进入私有日志。

为了让子 Run 可以独立成立，Analysis Session 在顶层 Workflow 开始前显式发布，而不再等待首个或父 Run 成功。空 Session 是持续合法状态；成功或失败的 `kat run` 都不自动删除用户已经创建的 Session，只有显式 Session 删除操作才结束其生命周期。任一子 Run 发布后，Session 与这些子 Run 同样在父级失败后继续存在。此项决定取代 Issue #248 与 ADR-0077 中“首个 Run 成功后才发布 Session、首次 Run 失败清理整个候选”的对应条款。

最外层 Host 从顶层 Workflow 启动前到整个嵌套 Runtime 树结束、全部已开始调用收拢且父 Run 发布或失败完成为止，持续持有该 Analysis Session 的共享执行租约。它覆盖子 Catalog 的 footer 验证和后续 DataFusion 查询，保证父 Runtime 生命周期内对应 Parquet 不会被并发删除。Session 整体删除必须取得独占租约；存在活动执行时快速返回“Session 正在使用”，不等待、不取消或终止 Workflow。测试临时 Session 同样只在最外层 Runtime 完全退出并释放文件句柄后清理。

顶层 `kat run` 结束后不把这份共享租约延长到 AI 推理或下一条命令。每次 `kat query`、`kat inspect session` 和按双 ID 执行的 Workflow inspection 都为自身操作重新取得共享 Session 租约并在结束时释放；Session 可能在两条命令之间被显式删除，后续操作按不存在安全失败。KAT 不为了连续分析建立跨命令或任务级租约。

组合 Workflow 仍必须像叶子 Workflow 一样返回 `dp.Table | dict[str, dp.Table]`，并把这些父级结果完整写入自己的 Run Output；`dp.Catalog` 不能直接作为 Workflow 返回值，也不会把子 Output 自动重发布、复制或转成父级逻辑引用。需要固化但只负责依次调用子 Workflow、随后由父 Guide 汇总子级解释的组合 Workflow，可以返回一个具有明确非空 Schema 的零行 Table；它仍是普通 Output，不引入无 Output Run、特殊编排结果或固定状态表 Schema。只需保留子证据时，已发布的子 Run 本身就是查询边界；父级只有形成新的组合结果时才读取所需数据并承担父 Output 的写入成本。

组合 Workflow 成功返回并完成全部 Output 验证后发布一个与叶子 Workflow 相同的普通 Run。其 Run Manifest 由 Host 根据持久发布事实记录本次执行实际成功发布的全部直接子 Run，包括只参与中间计算而未被父级返回，以及发布后 Catalog 交付失败但被父级捕获的子 Run。`child_runs` 按 Run ID 稳定排序，但语义上只是无序引用集合，不表达调用先后、分支或线程关系；它也不记录预期调用、失败候选、步骤状态或执行拓扑。嵌套组合只在各层记录直接关系，由 Manifest 链表达更深层调用。

失败 Workflow 不发布 Run 或 Manifest，因此不能承载到其成功后代的关系。例如 `A → B → C` 中 C 发布后 B 失败，而 A 捕获错误并成功时，C 继续作为同一 Session 中的已发布 Run，但既不拥有失败 B 的持久父边，也不会被提升为 A 的直接子 Run；A 的 `child_runs` 不包含 B 或 C。Session inventory 仍能列出 C。保留失败层级需要持久 invocation/执行图，超出本决策边界。

`kat run` success Response 继续只返回本次顶层 Run 的 Session ID、Run ID 与 Output inventory，不递归展开调用树。顶层父 Workflow 失败时 failure Response 也不返回部分 Run 列表；Skill 已知显式 Session ID 后可以重新 inspection 其中的已发布事实，但在同一 Session 存在并发执行时，KAT 不保证把没有已发布父级的 Run 精确归因到某一次失败调用。

## Authoring examples

顺序组合直接使用子 Catalog 的真实 relation name，并把前一步查询得到的普通标量传给下一步：

```python
def analyze(ctx, trace: str):
    summary = ctx.run("trace-pack", "summarize", trace=trace)
    selected = dp.DataFusionProvider(catalog=summary).query(
        "SELECT thread_id FROM main ORDER BY cpu_ns DESC LIMIT 1"
    )
    thread_id = selected.to_arrow()["thread_id"][0].as_py()

    detail = ctx.run("thread-pack", "inspect", trace=trace, thread_id=thread_id)
    return dp.DataFusionProvider(catalog=detail).query(
        "SELECT * FROM findings WHERE severity >= 2"
    )
```

确有独立子调用时，作者可以使用标准线程库；`Future.result()` 同时等待并把线程异常带回父入口。多个 Catalog 先分别缩减，再用内存 Table 融合：

```python
def compare(ctx, left: str, right: str):
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
        left_future = pool.submit(ctx.run, "trace-pack", "summarize", trace=left)
        right_future = pool.submit(ctx.run, "trace-pack", "summarize", trace=right)
        left_catalog = left_future.result()
        right_catalog = right_future.result()

    left_table = dp.DataFusionProvider(catalog=left_catalog).query("SELECT * FROM main")
    right_table = dp.DataFusionProvider(catalog=right_catalog).query("SELECT * FROM main")
    return dp.DataFusionProvider(tables={"left": left_table, "right": right_table}).query(
        "SELECT * FROM left UNION ALL SELECT * FROM right"
    )
```

只需要固化调用关系、把解释留给父 Guide 的组合 Workflow 仍返回一个普通零行 Table，而不是特殊编排结果：

```python
EMPTY_SCHEMA = pa.schema([pa.field("completed", pa.bool_(), nullable=False)])

def collect_evidence(ctx, trace: str):
    ctx.run("cpu-pack", "analyze", trace=trace)
    ctx.run("io-pack", "analyze", trace=trace)
    return dp.Table.from_rows([], schema=EMPTY_SCHEMA)
```

这些片段只表达组合边界；声明装饰器、imports 与 PACK 布局继续遵循现有作者合同。

## Consequences

- 普通顺序代码是首要作者路径；需要并发时使用 Python 标准线程能力。
- KAT 不引入 Markdown-only、Agent-executed 或其他第二种 Workflow kind。
- Guide 只解释所属 Run；Skill 从父级开始并按最少证据原则惰性展开相关子 Run，Guide 不自动合并，缺省 Guide 也不制造解释任务。
- Session inspection 只增加每个 Run 的直接 `child_runs`，复用既有双 ID Guide inspection 与 Query 完成惰性遍历。
- Guide 驱动的后续 Run 是同 Session 的新根 Run，已发布父 Manifest 不会因解释阶段继续分析而改变。
- 临时分析不产生需要命名、版本化、恢复或清理的编排对象；已有子 Run 是其唯一持久执行事实。
- 空 Session 是持续合法状态；Run 执行不拥有其删除权，Session 只通过显式整体删除结束。
- 最外层执行对 Session 持共享租约，整体删除需独占租约且遇到活动执行快速失败。
- KAT 只保证单次 `ctx.run()` 的隔离、发布与线程安全，不把用户线程组织提升为持久执行拓扑。
- 工作线程错误只有通过 `Future.result()` 或显式重抛才影响父入口；普通 `join()` 不改变 Python 的异常语义。
- 并发子 Run 复用 Issue #248 的 candidate 隔离与 materialization no-replace 合同，不以 Session 锁串行化。
- 父函数返回不是后台编排的分离点；仍有活动调用时父 Run 必须失败，Host 只负责收拢已经开始的调用。
- 活动调用链检查排除递归与循环调用，同时不限制顺序重复调用，也不引入深度配置。
- `child_runs` 只保留直接调用事实；稳定序列化不把调用顺序或并发关系提升为持久合同。
- 失败层级不会补写或压平；其成功后代留在 Session inventory，但不获得虚构的祖先关系。
- 不存在需要与 Python 控制流同步维护的静态子 Workflow 声明或执行图。
- 不存在与真实 Parquet 并行维护的静态 Workflow Output Schema；Catalog、SQL planning 与组合集成测试构成消费合同。
- 组合调用不会把另一个 PACK 的代码挂载进父 Runtime，也不建立 PACK 之间的 Python import 依赖。
- 嵌套 PACK discovery 固定继承顶层命令范围，Workflow 代码不能用路径参数扩大它。
- `ctx.run()` 始终返回一个子 Run Output Catalog，不返回 eager Table、Run handle、Output reference 或 query facade。
- Catalog 复用已经发布的 Parquet，父级只在显式 DataFusion query 时读取所需数据，不产生额外 Arrow IPC 文件或整表内存传输。
- 嵌套调用只增加复用标准流的私有 JSONL 控制 RPC；Host 持续服务请求并最后等待 Runtime，避免递归 CLI 和 wait-first 死锁。
- Host 持有 Session、发现范围、调用链和父子 ledger；Python 只发送调用意图并接收可构造 Catalog 的已验证 relation 描述。
- 多子调用先分别缩减各自 Catalog，再走已有的内存 Table fusion；首版没有多 Catalog namespace 或联邦查询。
- 父 Workflow 只发布自己新形成的 Table，不把 Catalog 或子 Output 引用提升为第二种 Run Output。
- 纯调用型但需要固化的组合 Workflow 可以发布显式 Schema 的零行 Table，不放宽“Run 至少一个 Table Output”的不变量。
- 子 Run Catalog 供父级按需查询，不成为下一次 `ctx.run()` 的特殊输入类型。
- 两个仅限位置的路由参数保持调用简洁，也避免与目标 Workflow 的具名输入发生名称冲突。
- 嵌套输入是封闭的严格 Python 标量合同；带类型 RPC 只传输值，目标 Input Compiler 仍是默认值与有效性的唯一权威。
- PACK test 复用生产嵌套执行语义，但以测试临时 Session 隔离且不把记录写入生产存储。
- 跨 PACK 测试只使用精确被测 PACK 与正常已安装 discovery roots，不新增 sibling checkout 参数。
- 子 Run 的首次 Parquet 写入仍是其独立发布成本；父级查询明确承担所需的 Parquet 读取，父级新结果也按普通规则写入。
- 组合 Workflow 不形成跨 Run 事务；父级失败不能抹除已经成立的子 Run 事实。
- 子 Run 发布与 `ctx.run()` Catalog 交付是两个完成点；后者失败不改写前者。父 Run 最终发布时，该子 Run 仍写入其 `child_runs`；父 Workflow 失败时，子 Run 只保留在 Session inventory 中，不保留父子关系。
- `kat.RunError` 只提供稳定异常类型，不成为可解析状态机；所有主动重试均按可能重复执行处理。
- 父 Run 是可查询结果与实际调用事实的发布边界，不是计划、调度或事务容器。
- Session 的存在只表达共同分析边界，不证明顶层或父 Workflow 成功。
- 这是早期阶段的破坏性合同变更：Manifest writer/reader 同步要求 `child_runs`（叶子写空数组），不迁移旧 Session；ADR-0077 与 Issue #248 的命令、Skill reference 和测试必须在实现时一并改为显式 Session 创建与必填 `--session`。
