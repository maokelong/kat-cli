---
status: accepted
---

# Workflow 参数语义只属于 Python Runtime

> ADR-0062 已从 `@kat.workflow` 与 inspection 中删除 `required_tables`，并删除由其驱动的 Dataset/Table Grant 条件分支。`kat run --dataset` 仍可选且没有隐式默认；省略 Dataset 或缺少 Binding 时只在对应 Source namespace 首次实际解析时失败。本文其余 Workflow Input Compiler、参数与 Run Manifest 决定继续有效。
>
> Source Input Compiler 只复用该 Compiler 的私有编译内核和现有标量语义，并以 Source-only profile 增加 `pathlib.Path` 与表示可重复多文件 option 的 `tuple[pathlib.Path, ...]`。Binding 与 `kat_run(sources=...)` 的原始 argv 都是 KAT 实际收到、已经由 shell 或调用方完成分词和可能变量展开的 token strings；KAT 不再次展开环境变量、模板或 response file。本文对 Workflow `Path`、容器与自定义 parser 的拒绝保持不变。

`kat run` 用 `--` 分隔 KAT 的固定路由参数与 Workflow arguments；Rust CLI 将后者作为原始字符串数组写入 `run_workflow` request，不猜测参数类型、不应用默认值，也不根据 Workflow 动态构造 Clap Interface。`--dataset` 同样只是始终可选的固定路由参数：提供时 CLI 通过 Dataset Storage 解析，省略时原样表达缺席；CLI 不读取 Required tables 来改变参数必填性。这样 CLI 像透明调用一个 Python CLI，仍由自身独占内部 `request.json` 的生成权，同时消除 Rust 与 Python 两套参数语义以及 `"true"`、`"001"` 等启发式转换歧义。

Workflow Runtime 从实际函数签名与 KAT Workflow decorator 生成唯一的 Workflow Input Compiler，并用同一份规范化 KAT 输入约束共同驱动 PACK inspection、生产 `kat run` 与 PACK test `kat_run` 的 Workflow 执行；PACK 作者不能另写 CLI parser。Runtime 选定 Workflow 后使用 decorator 已复制规范化的 Required tables 校验可选 Dataset，并在创建 Workflow execution plane 前解析 arguments、补齐默认值、构造领域类型和 Table Grant：非空依赖缺少 Dataset 时失败，空依赖则无论是否提供 Dataset 都继续；成功后调用 `workflow(ctx, **effective_inputs)`，Run Manifest 记录规范化输入。Workflow 输入只承载少量单次运行控制选择；数据属于 Dataset，稳定策略属于 PACK，因此第一版不引入 params file、通用 JSON Schema、Pydantic model 或复杂嵌套输入。

合法 Workflow callable 只能是当前入口 module 顶层以普通同步 `def` 定义的 Python function，不接受 nested function、method、lambda、callable object、`async def`、generator 或 async generator。第一个参数必须是名为 `ctx`、无默认值、解析后标注精确为 `kat.Context` 的 positional-or-keyword 参数，Runtime 只把它作为第一个位置参数传入。其余用户参数可以是 positional-or-keyword 或 keyword-only，Runtime 始终按名称传入；positional-only、`*args` 与 `**kwargs` 在生产 Interface 加载阶段拒绝。

Bundled Python 精确锁定 3.14，因此 Workflow 输入标注遵循 Python 3.14 原生延迟求值语义。Input Compiler 使用标准库 `annotationlib` 而不直接读取 annotation 源码或自建类型语法；它在 Workflow function 的定义作用域内只解析 `ctx` 和用户输入，接受可解析的 forward reference，再要求每个结果精确落入 KAT 封闭类型集。任一输入标注无法求值或结果不受支持时，诊断点名参数并使整个生产 Interface 加载失败，不退回到字符串猜测。

Workflow return annotation 可以按普通 Python 惯例省略或书写，但 KAT 不求值、不校验、不展示，也不把它当作 Output contract；Input Compiler 不得因为批量解析整个 annotation mapping 而让不可解析的 return forward reference 导致 inspection 失败。真正调用后的返回值只由 `kat run` 的 Workflow 返回边界按既有 DataFrame/具名 DataFrame map 规则校验；inspection 不为尚未执行的 Workflow 推断 Output。

唯一作者调用形状是 `kat.workflow(*, name, title, required_tables, parameters=None)`。Decorator 必须带括号且只接受关键字；`name`、非空 `title` 与 `required_tables: list[str]` 始终必填，无 Dataset 依赖也显式写空 list。存在任一非 `ctx` 用户参数时必须提供 `parameters: dict[str, str]`，没有用户参数时可以省略。Decorator 对展示用的 title 与参数说明 value 使用 Python `str.strip()` 去掉外层 whitespace，清理后为空即失败，公开 Interface 使用清理值且保留内部空白与换行；name、parameter key、Required tables、choices 与 default 等机器值不参与处理。Decorator 应用时复制这些容器并生成不可由 Workflow 修改的私有规范约束。不接受裸 `@kat.workflow`、位置实参、`description=`、tuple 形式的 Required tables 或未知扩展参数。

参数类型、default、required 与 choices 只来自函数签名；每个参数的自然语言说明集中写在 Workflow decorator 的 `parameters: dict[str, str]` 中。该映射的 key 必须与全部非 `ctx` 签名参数精确一致，value 经 `str.strip()` 后必须非空并以清理值发布，展示顺序服从函数签名；无用户输入的 Workflow 可以省略它。新增、删除或重命名参数后若未同步映射，生产 Interface 加载以 missing 或 unknown parameter 直接失败。这点受控的名称重复换来普通、清晰的 Python 签名，并由确定性校验消除漂移风险。Workflow description 的唯一来源仍是函数 docstring，decorator 不提供同义字段。

函数签名中的 raw default 不建立一套独立于 Click 的精确类型兼容规则。Compiler 先把标注编译成 Click `Option` 与对应 `ParamType`，再让 raw default 和显式 argv 共同经过该参数的同一条 Click 转换与校验路径；例如 `ratio: float = 1` 的 effective default 是 `1.0`，`window: kat.Duration = "5ms"` 也由 Duration `ParamType` 构造成领域值。PACK inspection 与 Workflow execution 必须使用完整 Command 解析结果，或依次调用 Click 公开的 `get_default()` 与 `type_cast_value()`，不得直接发布或传入 raw default；转换失败就以同一参数诊断使完整 Interface 失败。Runtime 只用 Click 产生的 effective values 调用 Workflow，普通 Python 直接调用函数并绕过这条路径不属于 KAT Workflow invocation Interface。

Workflow 自身的 description 使用函数的非空普通 Python docstring，并作为该字段的唯一来源；decorator 不重复声明 description。Runtime 依次使用标准库 `inspect.cleandoc()` 与 `str.strip()` 清理惯例缩进和外层 whitespace，以清理后的全文发布 description，内部空白与换行保留，但不解释 `Args:`、Google、NumPy 或 Sphinx 等结构化方言。生产 Interface 加载拒绝缺失或清理后为空的 docstring，Bundled Runtime 也不使用会删除 docstring 的 `python -OO`。这保留了 IDE 和 Python 开发者熟悉的文档位置，同时没有建立第二套参数描述语法。

除第一个 `ctx` 外，每个 Workflow 用户参数都确定性地得到一个 Click long option：Compiler 按 Click 文档推荐的常规形式给精确 Python 参数名添加 `--` 并把 `_` 替换为 `-`。非 bool option 无默认值时必填、有默认值时可选；bool 使用 Click 原生 `--name/--no-name` pair，省略时采用函数默认值，不接受 `--name true` 或 `--name false`。第一版不开放手工 option name、短选项或别名，但也不为自动生成名称建立 KAT 正则、ASCII/lowercase 限制、`no_` 禁令、保留名称或冲突表。私有 Command 显式设置 `add_help_option=False`；因此 `help` 不是保留参数名，`--` 后也不存在“请求 Workflow 帮助”的第三种结果。其余完整 Command 的名称接受、重复参数 warning/failure 与解析行为完全由原子包精确锁定的 Click 决定；KAT 不把 Click 接受或只警告的形态升级为自己的 failure。

第一版 Workflow argv 不支持位置参数；这不限制 Python function 的用户参数采用 positional-or-keyword kind，因为 Runtime 始终以关键字参数调用它们。函数签名、inspect、参数解析与实际调用都使用同一组 Click Option，避免出现第二套名称或冲突语义；inspection 的 parameter array 仍以签名顺序提供稳定展示，并直接发布 Click Command 实际使用的 `option` 与 bool `negative_option`。固定 KAT 路由帮助只由分隔符前的 `kat run --help` 提供，Workflow 说明的唯一机器界面是 PACK inspection。

`kat inspect --pack` 的成功 KAT Response 直接把 PACK Interface 放在 `result`，不再增加 `pack` wrapper。CLI 从本次 Discovered PACKs 中已选中的 PACK 取得 `name`、`title`、`description` 与 `owner`，只把所选 PACK 的 name 和 canonical path 交给 Runtime；Runtime 不读取 manifest，Runtime Response 的 success `result` 只返回完整的 `workflows`，CLI 验证后再合成公开 object。每个 Workflow 包含 `name`、`title`、`description`、去重后的 `required_tables` 与 `parameters`。Workflow 按 name 排序，Required tables 按 name 排序，parameters 保留函数签名顺序；没有发现 Workflow 或参数时对应字段返回空 array，不省略。缺席或没有 `.py` 入口的 `workflows/` 得到空 `workflows`，是完整 Workflow discovery 的诚实结果并仍然成功；一旦存在 `.py` 入口，它就必须恰好注册一个本 module 定义的 Workflow。只有扫描/读取、manifest、导入、入口或声明等错误使 KAT 无法形成可信的完整 Interface 时才失败，并且任一入口失败都会使整个 inspection 失败。此时公开 failure Response 不含 `result`，也不返回 manifest-only PACK、已成功发现的 Workflow 或 completeness 标记；Skill 自行把成功的空 PACK 排除在分析候选之外，KAT 不把“没有能力”混同为“检查失败”。

每个 parameter object 固定包含精确 Python 参数 `name`、Compiler/Click Command 实际使用的 `option`、`type`、`required` 和 `description`。bool 另外包含表示 false 的 `negative_option`；其他类型不出现该字段。`type` 是语言无关的封闭集合 `string`、`int64`、`float64`、`boolean`、`duration` 与 `wall_clock_timestamp`，字符串 `Literal` 仍使用 `string` 并额外携带非空 `choices`。Compiler 把 `Literal` 视为值集合：校验所有成员都是字符串，去重后以 Bundled Python 普通 `sorted(str)` 顺序稳定输出，并以同一规范化列表构造 Click Choice；不保留源码参数顺序或增加 order 字段。必填参数省略 `default`；可选参数始终携带 `default`，包括 `false`、空字符串与允许的 JSON `null`。默认选择只由 `default` 表达，`choices[0]` 不具有推荐或优先语义。Skill 使用 `option` 传递一个值 token，或为 bool 直接选择 `option` 与 `negative_option`，不再实现参数名映射、反向 flag 或 Python 类型到 CLI 的推导。

`default` 投影 Click 已转换并校验的 effective default，而不投影函数签名中的 raw object：`str` 与字符串 `Literal` 是 JSON string，有符号 64 位 `int` 是无前导 `+` 的十进制 JSON string，有限 `float` 是 JSON number，bool 是 JSON boolean，`kat.Duration` 是 temporal `ParamType` 保留的合法 CLI literal，`kat.WallClockTimestamp` 复用规范 UTC RFC 3339 formatter，允许的 `T | None = None` 是 JSON `null`。这与 Query Result 对 64 位整数的无损规则一致，也不会让 Python object repr 泄漏进 Interface。`default` 只说明省略 option 时 Runtime 会采用的值；Skill 没有覆盖时必须省略该 option，不把 inspection 中的默认值重新编码后传回。

该 inspection 不暴露 Python module、function、signature、type spelling、Click object、help 文本或 JSON Schema，也不重复整条 `kat run` 命令。PACK、Workflow 与 Dataset 的路由属于固定 KAT CLI，inspection 只发布 Workflow 自己的调用约束；默认值也只是说明省略 option 后 Runtime 将采用的事实，不要求 Skill 把它重新传回 CLI。

Workflow Input Compiler 使用原子 Payload 中精确锁定的 Click 作为私有参数语义引擎，并只依赖其公开的程序化 `Command`、`Option`、`Context`、`ParamType` 和异常 Interface。KAT 只负责把受支持的 Python 标注和领域边界编译成这些 Click 参数；Click 独占签名默认值与 argv 的取值、转换、choices、必填校验和解析错误语义。Runtime 根据同一组 Click 对象生成 inspection effective default、解析给定字符串数组并形成 effective values，再把 Click 异常转换为封闭的 Runtime Diagnostic；不得在 Click 前后再实现 raw-default 类型表或另一套数值拓宽规则。私有 Command 不启用 env、`default_map` 或 prompt，因此取值来源只有显式 argv 与函数签名默认值。Click 不成为 PACK 依赖或作者 Interface：PACK 不使用 Click decorator、annotation、类型或 parser，PACK inspection、Runtime Response、Run Manifest 与日志也不暴露 Click 对象和异常名称。Typer 会引入第二套函数签名解释与 CLI 作者语义，`argparse` 嵌入 Runtime 时还需由 KAT 接管退出、help 和更多错误路径，因此第一版均不采用；KAT 不为一个私有实现预设可替换 parser port。

KAT 不把 Click 的完整能力变成 Workflow Interface。第一版只接受 `str`、有符号 64 位范围内的 `int`、有限 `float`、有默认值的 `bool`、`kat.Duration`、`kat.WallClockTimestamp` 和字符串 `Literal`；bool 使用 Click 原生正反 flag 行为，整数范围、有限浮点数与两个时间类型的封闭语义都实现为 Click `ParamType`/range，使 default 与 argv 经过同一个转换器。`T | None` 只允许包裹其中的非 bool 类型且必须以 `None` 为默认值，省略选项是得到 `None` 的唯一方式。`Literal` 是 choices 的唯一来源，其参数按 Python 的集合语义去重排序；default 与 required 只来自函数签名，decorator 不重复声明它们。缺失标注、`Any`、其他 Union、Enum、Path、Python 时间类型、容器、结构化 model、PACK 自定义类型与自定义 parser 在生产 Interface 加载阶段拒绝。这个封闭集合全部由 Click 的公开 Option 与 ParamType seam 实现；Click 后续增加能力不会隐式扩大 KAT 的作者约束。

第一版不把参数说明放进 `Annotated[T, kat.Param(...)]`，因为说明文本不值得让每个类型标注长期承担视觉噪音；也不解析结构化 docstring，因为那会额外建立一套公共作者语法并引入方言漂移。decorator 映射已经是 Workflow 元信息的既有位置，且严格 key-set 校验把字符串键的重命名风险限制在生产 Interface 形成阶段。
