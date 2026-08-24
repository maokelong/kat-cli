---
status: accepted
---

# Hitrace 区分未知扩展与损坏数据

> ADR-0062 已由 Hitrace Source Entry 取代平台 Data Import 作为该失败语义的最终拥有者；ADR-0063 又删除 `kat import hitrace` 及其 operation-specific 成功 result，因此下文 `path` 与两个 `unsupported_*` arrays 不再是公开 Response 合同。unknown content 与 corrupt data 的区分、已承诺内容损坏时整体失败以及不发布部分表的原则继续有效。

Hitrace Datasource 遇到合法但未注册的 plugin 或未知 section type 时继续导入已支持内容，不为未知内容生成 Trace fact 表。成功的 `kat import hitrace` operation-specific `result` 始终包含三个字段：`path` 是最终 Dataset 的 canonical 绝对 Unicode 路径，`unsupported_plugins` 是去重并按名称排序的 plugin string array，`unsupported_section_types` 是去重并按数值排序的 section type integer array；后两项没有对应未知内容时仍返回空 array。这样 Skill 既能继续 inspect 并使用确切 Dataset，也无需解析日志就能区分“没有发现未支持容器内容”与“Dataset 只覆盖来源的一部分”。Skill 根据用户问题决定是否提示或继续，KAT 不猜测相关性。

每种未知内容的出现次数、文件位置和解码技术细节只写入 Operation log，不重复占用结构化结果。KAT 不把这些短命导入覆盖事实写入 Dataset、catalog、manifest 或其他 metadata，也不为它们建立通用 `warnings`、severity 或错误码体系。Hitrace 容器、framing 或已注册 plugin 的解码一旦失败，整个 Data Import 立即失败且不发布部分 Dataset；未知能力不是损坏，已承诺能力也不采用 best-effort。
