---
status: accepted
---

# Run 通过路径引用 Dataset 当前内容

> ADR-0062 与 ADR-0063 保留 `kat run --dataset` 可选、显式路径身份和 Run 不保存历史 Dataset 快照的决定；省略时仍不选择或构造隐式 Dataset。它们把可用 Dataset 从匿名 `dataset.*` 改为按 PACK catalog 与 Source schema 注册的 External/Materialized Bindings，并把 External Provider 的取得推迟到对应 Source namespace 首次实际解析。一次 Run 或 Dataset Query 仍只选择一个 Dataset；比较两份来源数据应由一个 Source Provider 在同一 Binding 中将差异显式建模为字段，不增加多 `--dataset`、alias、merge 或 overlay。完整目录复制产生以新位置为身份的独立 Dataset；移动或重命名 Dataset 是未定义行为，旧 Run 尤其不承诺自动跟随，KAT 不提供 relocation 或引用改写。

Dataset 没有独立 ID；其文件系统 canonical 绝对 Unicode 路径是 KAT 唯一记录的身份。Dataset Storage 使用成熟的平台路径能力，在解析已有 Dataset 时 canonicalize，在新 Dataset 创建后 canonicalize；它不手写 `.`、`..`、Windows 前缀、盘符或大小写规则。用户提供的 Dataset 根本身可以是平台能够正常解析的 symlink、junction/reparse alias 或挂载路径；KAT 不先分类或拒绝这些形态，而是验证解析后的目标目录与其中的直接普通 marker，返回和记录 canonical target path，不保留输入别名。悬空或无法 canonicalize 的路径自然失败。第一版只承诺本地目录；网络共享、device path 或其他特殊位置不增加识别、拒绝、URI 转换或兼容层，其底层行为不属于 KAT Interface。

用户给出的相对 Dataset 路径以调用 `kat` 进程的当前工作目录为基准，不相对 Skill、binary、KAT Data Home 或 PACK。已有 Dataset 在解析时、新 Dataset 在创建成功后转换为 canonical absolute Unicode path；无法取得 cwd、无法 canonicalize 或无法无损表示都形成当前操作 failure，不使用 `to_string_lossy()`、cwd fallback 或输入拼写作为持久身份。

`kat import` 未指定目标时仍以 UUIDv7 生成不冲突且可排序的目录名，但该值只属于路径，不重复持久化或形成 Dataset ID。`kat run` 的 `--dataset <dataset-directory>` 可省略；显式提供时 CLI 定位并验证 Dataset，并把 canonical path 写入最终 `manifest.json`，省略时该可选 path 字段直接缺席，不增加 `has_dataset` 或空值。该文件仍是后续查询唯一读取的 Run 事实，临时 Runtime Response 与原 request 都不作为恢复来源。Run 有 Dataset reference 时，Output Query 由 Rust Dataset Storage 重新解析该路径，成功则注册 `dataset.*`，失败仍允许健康的纯 `output.*` 查询；Run 没有 Dataset reference 时不注册 `dataset.*`，也不让 Python 重读 Run Manifest、解释 Dataset 文件树或构造隐式 Dataset。成功 Query Result 始终以 `dataset` tagged object 明确投影这三种事实：没有 reference 为 `not_provided`，当前可解析为 `available`，有 reference 但当前不可解析为 `unavailable`。

Data Import 整体覆盖、用户删除后重建同一路径，以及其他使该路径内容变化的操作，对旧 Run 都具有相同语义：`dataset.*` 读取查询时的当前数据，`output.*` 保持运行时结果。路径当前内容与旧 Run Output 中换算得到的目标 `ClockValue`，或其后经严格 cast 派生的 `Timestamp(ns, UTC)`，可能不再具有共同时间语义；KAT 不保存用于证明二者时间一致性的 token 或 generation，不警告或阻止用户在同一 Output Query 中比较它们，跨 Dataset 时钟一致性由调用者负责。Output Query 仍可调用 `kat_convert_clock`，并只使用查询当下的当前 Dataset 证据；Runtime 不区分参数来自历史 `output.*` 还是当前 `dataset.*`，路径覆盖后的语义后果同样由用户负责。路径缺失或标记与受管理表无效时只让 `dataset.*` 不可用，纯 `output.*` 查询仍可执行，但实际调用时钟换算会因没有当前证据而失败；移动或重命名 Dataset 不会让旧 Run 自动跟随。第一版不维护 Dataset ID、catalog、revision、snapshot、内容 hash、表达式 lineage 或运行时输入副本。
