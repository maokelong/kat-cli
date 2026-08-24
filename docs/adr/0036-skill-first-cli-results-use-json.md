---
status: accepted
---

# KAT Response 使用 JSON tagged union

> ADR-0062 已将 `kat inspect --dataset` 的成功 `result` 从根级扁平 `tables` 改为按 `(PACK identity, Source name)` 排序、按 Binding 投影的 `sources` tagged union；精确 `kat inspect --pack` 还投影可选 `source_guide` 与 Source Entries，并在存在任一 Entry 却缺失或无法读取 `SOURCES.md` 时失败。没有 Entry 且没有 Guide 时 `source_guide: null`。ADR-0063 删除所有 Data Import operation-specific results。本文其余 KAT Response 外壳、严格 operation-specific result 与 inspection 无日志决定继续有效。

KAT Skill 是产品 Interface 的第一公民，普通终端使用是第二公民，因此外层 KAT 参数一旦被 Clap 解析为具体操作，该操作无论成功失败都只向 stdout 写一个结构化 JSON document。这个短命 document 统一称为 KAT Response；它由 CLI 从当前操作事实生成，不直接透传内部 Runtime Response，不成为 manifest、catalog 或持久事实源，也不暴露 Output ID、物理布局或私有 IPC 字段。Skill 与 CLI 原子发布，字段可以在产品发布前随两者一起破坏式演进，不为 CLI 建立独立兼容承诺。

KAT Response 协议只从 Clap 产生具体业务操作后开始。根命令和各操作的 `-h`/`--help` 保持 Clap 原生解析期元动作：直接向 stdout 输出普通帮助文本并以 `0` 退出，不生成 Response，也不解析 Skill 根、访问 KAT Data Home、执行 discovery 或创建日志。当前切片不启用 `--version`，因为 workspace 的临时 crate version 还不是权威 KAT 产品版本；只有构建发布系统提供唯一版本源后才增加。裸 `kat` 没有表达业务操作，按缺少 subcommand 的 parse failure 处理，而不自动展示帮助并成功退出；裸 `kat inspect` 则已经表达“列出可发现 PACK”的有效操作，必须执行 discovery 并返回 KAT Response。未知命令、缺少或冲突外层参数等 parse failure 使用 Clap 原生 stderr 与非零退出，stdout 必须为空。因而 stdout 的语义由解析状态精确决定：显式 help 是普通文本，具体操作是且仅是一个 KAT Response，parse failure 没有内容；形成操作后不允许增加第二种 stdout 产品格式。

KAT Response 是以 `status` 为分支标记的封闭 tagged union。代码层以独立、封闭的 generic envelope `KatResponse<P>` 复用该形状，`P` 是 operation-specific concrete Skill-facing result 类型，不得以 `serde_json::Value` 或 `dict[str, Any]` 擦除类型；它与私有 `RuntimeResponse<R>` 不是同一个类型。success 分支精确包含 `status: "success"` 与操作专属 JSON object `result`，不含 `error`；failure 分支精确包含 `status: "failure"` 与 KAT diagnostic `error`，不含 `result`。只有操作自身定义并成功交付 Operation log 时，相应分支才额外包含顶层 `log_path`；日志未成立或文件不可读时省略。无目标 `kat inspect` 与 `kat inspect --dataset` 的成功与 failure 分支都不包含 `log_path`。`kat test` 的两个分支在 pytest 返回且 CLI 分配的报告路径最终是普通文件时都额外包含顶层 `test_report_path`，文件不存在时省略；这个证据字段不是 `result` 或 `error` object 的成员。`log_path` 与 `test_report_path` 在外壳中逐项显式列出，不抽象 `ResponseMeta`、通用 metadata/evidence bag 或 extension map。Rust 只需以 `KatResponse<P>` 序列化最终公开文档，不为自家输出建立反序列化协议。KAT 不使用 `error: null`、`result: {}`、`log_path: null` 或 `test_report_path: null` 作为占位符。顶层不重复 Skill 已经掌握的 operation，不增加独立 `schema_version` 或通用 timestamp。

无目标 `kat inspect` 的 success 分支示例：

```json
{
  "status": "success",
  "result": {
    "packs": []
  }
}
```

无目标 `kat inspect` 的 discovery failure 分支示例：

```json
{
  "status": "failure",
  "error": {
    "message": "..."
  }
}
```

KAT CLI 是公开 Response 的唯一最终组装者；operation-specific Response assembler 与拥有该操作生命周期和强制门的 application handler 共置，领域 Module 与 Runtime client 只返回 typed facts/error，永远不依赖 KAT Response。当前 `response.rs` 隐藏 `KatResponse<P>`、KAT Diagnostic、`RenderedDiagnostic`、serde/miette implementation 与 writer test seam，只以 `pub(super)` 向父 `lib.rs` 暴露字段私有的 opaque `PreparedResponse<P>` 及 `prepare_success(result)`、`prepare_cli_failure(miette::Report)`、`publish(prepared)` 三个 Interface。operation-specific assembler 先把领域事实显式投影为 concrete result，再调用前两者之一；application entry 把返回的 handoff 交给 `publish`。当前不预建 Runtime failure、Operation log、`test_report_path` 或未来 operation 的入口；真实 Runtime client 出现后也只放宽共享 Diagnostic value 必需的 crate 内可见性，不暴露 envelope 或 publisher implementation。该 handoff 只服务当前进程的最终 I/O，不是 `ResponseMeta` 或公共 metadata；共享 response Module 不导入 PACK、Dataset、Run 等领域 Module，library 应用入口也不建立中央 assembler registry、全操作 result enum、全操作 assembler `match` 或平行 orchestration layer。publisher 用成熟 `serde_json::to_vec` 在内存中完成 compact Response serialization，并在同一个私有 buffer 末尾追加一个 LF 形成唯一 stdout frame；随后才尽力写可选 final stderr projection，最后对整帧严格 `write_all` 并 flush stdout，正常发布时才从 Response 分支决定进程状态。操作调用点不接触 JSON framing；KAT 不增加公共 `JsonWriter`、JSON Lines 依赖或多文档 stdout abstraction。publisher 也不接触原始 Report 或重新解释 Diagnostic。

Response serialization 失败时 stdout 保持为空；stdout write 或 flush 失败时可能已经存在部分 JSON。两种 publisher failure 都只尽力向 stderr 报告并强制非零退出，不递归构造备用 failure Response、不重试、不写第二份 JSON，也不建立 publisher error code。最终诊断与操作明确要求的实时 stderr mirror 都是第二公民的人类投影，其写入失败不改变既定 Response，publisher 仍继续尝试 stdout；Operation log、Run Manifest 和 PACK Test Report 等 Response 已承诺的持久交付物不适用这条 best-effort 规则。stdout failure 后 KAT 不回滚此前已经发布的 Run、Dataset 或报告：文件系统与外部 pipe 没有共同原子提交边界，调用方必须把业务结果视为未知，并把缺失、非法或与退出状态不一致的 JSON 当作 KAT protocol failure。

handler 与其他调用点不手写序列化字段，也不存在从任意 `RuntimeResponse<R>` 到公开 Response 的 blanket `From`/`Into`、通用 conversion trait 或 generic merge。assembler 只接收 CLI-owned 与 Runtime-owned 且互不重叠的显式 typed 参数；Runtime 未知字段由严格解码直接失败，两侧字段集合必须在类型定义上互斥；assembler 不接受无类型 dict、通用 JSON 或两份 Response，也不实现覆盖或值层碰撞算法；普通类型构造只是其内部实现，不形成第三个 Module。成功分支可以先构造 typed candidate 以执行验证或最终字节限制，但全部强制门成功前不得向 stdout 发布 success Response。`kat run` 的公开 `result` candidate 还必须从同一个内存 typed Run Manifest 纯投影并可在 persist 前构造验证，只有最终 `manifest.json` 发布成功后才可写入 success Response；Data Import 的 `unsupported_*` 等短命操作事实不因此写入 Dataset，也不为其他操作发明统一的“发布对象”。failure `error` 使用 ADR 0037 唯一定义的稀疏 typed Diagnostic：最终阻止操作成立的强制门拥有唯一 Diagnostic，合法 Runtime failure 只在全部外层门成功时原值移动，后续 CLI-owned failure 则取代并丢弃它，二者不合并。失败过程中碰巧形成的中间数据不伪装成部分 `result`，也不建立通用 `details` extension object；某项操作确实需要机器可读的失败证据时，必须为该操作明确设计 failure error 形状。JSON 外形、严格解码与失败门由 contract tests 验证；DTO 独立、没有 blanket conversion 和类型擦除由普通 code review 验证，不为证明某个 trait 不存在而引入 compile-fail 框架，也不重新反序列化 CLI 刚生成的公开 JSON。

生产 `kat run` 在启动 Runtime 前预分配私有候选 UUID，并以 `<data-home>/logs/run-<candidate-id>.log` 创建 Operation log；只有成功发布后，该 UUID 才成为 `run_id`。成功 `result` 始终且只包含 `run_id` 与非空 `outputs`；它在 persist 前从同一个内存 typed Run Manifest 纯投影并完成验证，不重新读盘，也不直接使用 Runtime result；只有最终 `manifest.json` 成功 persist 后，才可把这个 candidate 写入 success Response。`outputs` 是以已发布 Output name 为 key 的 JSON object，每个 value 始终且只包含 `columns` 与 `row_count`；`columns` 复用 Query Result 按 Schema 顺序排列的 `{name, type}` object array，`row_count` 是完整 Output 总行数的非负 `u64` JSON number，零值不省略或改成 string。它不重复 Output `name`、PACK、Workflow、Dataset、inputs 或 `queryable`，不发布 Run/Runtime Response 路径、Output ID、物理布局或自动 Preview。失败 `kat run` 不含 `result`，也不发布候选 UUID。

成功 `kat query` 的 `result` 始终且只包含 `dataset`、`columns` 与 `rows`。`dataset` 始终报告 Run 的 Dataset reference 及查询当下状态：Run 未提供 Dataset 时精确为 `{"status":"not_provided"}`；记录的 Dataset 当前可用时精确只有 `status: "available"` 与 canonical `path`；当前不可用但纯 `output.*` 查询仍成功时精确只有 `status: "unavailable"`、Run 记录的 `path` 与可读 `cause`。这个 object 不因 SQL 只访问 `output.*` 而省略，也不增加重复的 `current` 字段。失败 query 不含 `result`；只有 Runtime 对 `dataset.*` 或依赖 Dataset 证据的能力执行类型化解析，并由 `not_provided` 或 `unavailable` 状态直接选择失败分支时，相关状态、路径、cause 与行动建议才进入 Diagnostic。普通 SQL、Output、函数或其他 DataFusion failure 即使碰巧伴随该状态也不得附加 Dataset 上下文；KAT 不解析 SQL 或错误字符串猜测因果关系。

成功 `kat test` 的 `result` 精确只有 `summary`。`summary` 是 pytest category name 到正数计数的 JSON object：category 由 pytest 公开的 `pytest_report_teststatus` 决定，KAT 不重建分类；零值不发布，key 稳定排序。私有 `test_pack` Runtime 只用 pytest 公开 ExitCode 选择 Response 分支，不序列化原始 ExitCode：`OK` 时 success `result` 精确只有同形 `summary`，其他值时 failure `error` 不携带 `result` 或 partial summary。CLI 解构私有类型并新建公开 KAT Response；测试失败时 failure diagnostic 的 `message` 只表达 ExitCode 已确定的失败类别；完整 pytest terminal report 及其中可复制的失败 node ID 只出现在实时 stderr 与 Operation log。pytest 内建 JUnit XML reporter 另外生成逐测试 PACK Test Report；同一次测试以共享的私有随机 token 关联 `<data-home>/logs/test-<token>.log` 与 `<data-home>/test-reports/test-<token>.xml`，但不发布 Test Run ID 或建立测试 registry。pytest 返回后，CLI 只在预分配路径是普通文件时让两个 Response 分支以顶层 `test_report_path` 引用 XML；文件不存在时省略，pytest `OK` 但文件不存在则操作失败。KAT 信任 pytest 的报告，不解析 XML、验证 JUnit schema、核对 summary 或增加内部完成字段；也不解析 terminal report、读取 `TerminalReporter.stats`、增加 `error.test_report` 或把逐测试内容塞进 compact `result`。

Operation-specific `result` 只加入 Skill 完成当前任务所需的成功产品事实。每种 Data Import 都以 `path` 返回最终 Dataset 的 canonical 绝对 Unicode 路径，不重复 tables 或 Schema；从首次交付即标为 `Deprecated` 的 Trace Streamer import 仍精确只有该字段，弃用说明只放在 CLI help 与 KAT Skill，不为它增加通用 warnings 字段或成功 stderr。Hitrace import 还固定包含去重、稳定排序的 `unsupported_plugins` 与 `unsupported_section_types` arrays，即使为空也不省略。无目标 `kat inspect` 的成功 `result` 固定为 `{"packs":[...]}`；数组按 PACK name 排序，每项恰好包含 `name`、`title`、`description` 和 `owner`，未发现任何 PACK 时仍成功并返回 `{"packs":[]}`。它的完整事实或 discovery failure 都已经由 Response 表达，因此不创建 Operation log、不返回 `log_path`，也不为了空结果创建 KAT Data Home 或 `logs/`。`kat inspect --pack` 的成功 `result` 直接放置完整 PACK fields。`kat inspect --dataset` 的成功 `result` 直接是 `{"path":"...","tables":[...]}` Dataset object；其完整结果或 Dataset Storage failure 同样不创建 Operation log、不返回 `log_path`，也不创建 KAT Data Home 或 `logs/`。任一 inspection 失败都不含 `result`，也不返回其他合法 PACK、manifest 或部分 Workflow。

公共 KAT 进程状态只使用粗粒度三态。显式 help 或业务 success 为 `0`；已经形成的操作 failure，以及最终 Response serialization、stdout write 或 flush failure 为 `1`；未知命令、裸 `kat`、缺少或冲突外层参数等 Clap parse failure 为 `2`。对于已经形成的操作，退出码 `0` 与 success 分支、退出码 `1` 与 failure 分支必须分别同时成立；两者不一致、分支必填字段缺失、出现另一分支字段、未知字段或字段类型错误都属于 KAT 协议故障。未知 PACK、无效 Dataset、Runtime 失败和 `--` 后由 Click 拒绝的 Workflow 参数都发生在操作形成之后，返回 failure KAT Response。Clap parse failure 尚未形成操作，只向 stderr 报错且 stdout 为空；原生 help 的 `0` 只表示帮助成功展示，不属于 success Response。领域 Module 不分配其他退出码，Skill 需要的具体失败原因只由结构化 Diagnostic 表达；这套进程三态不扩张成稳定公共错误码目录。

CLI 独占 stdout；操作定义 Operation log 时，CLI 也独占该文件。已经承诺的日志创建、任一次写入或最终 flush 失败都会使操作进入 failure 分支；CLI 终止仍在运行的 Runtime、完成回收并拒绝发布业务成功，不做 best-effort 继续。部分日志仍可读取时 failure Response 保留 `log_path` 并由 diagnostic 说明日志不完整；文件不可用时省略 `log_path`。对生产 `kat run`，只有 Runtime 进程、Runtime Response、Operation log 和 Run Manifest 持久化全部成功后才发布 Run。外部强制终止进程时没有最终 KAT Response，日志和候选残留只供手工排障。

KAT 第一版不提供 `--json`、`--text`、`--format` 或 `--output`：JSON 已是唯一 Response。publisher 始终输出一行没有无语义内部空白的 compact serialization，并以单个 LF 终止；不使用平台相关 CRLF、BOM、提示前缀或额外空行，pretty JSON 只用于文档示例。
