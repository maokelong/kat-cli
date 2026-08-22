---
status: superseded by ADR-0079
---

# AI 按选定数据源延迟加载 Data Source Knowledge

> 本文已按 ADR-0069 修订 Data Source Knowledge 的独立共享部署边界。

Source Knowledge Package 开发期的 AI 可以从源码视图读取完整 Data Source Knowledge；正常分析期则先依据彼此独立的有界目录选择 PACK、Workflow 和数据源，再只加载所选数据源的说明与查询资产。KAT 不把所有 PACK、所有数据源的完整数据库知识预先注入模型上下文；同一份版本化知识服务开发与分析两个阶段，但必须通过明确的静态发现与读取 Interface 暴露，单纯约定一个未被 KAT 解释的资源目录不足以形成分析能力。

现有 `kat inspect --pack` 继续只返回 PACK 与 Workflow Interface，不嵌入数据源目录。分析期只有在已经选定 PACK 且确实需要数据源知识时，才调用独立的 source inspection 操作取得紧凑 Data Source Manifest 列表；完整知识仍需在选定单个数据源后定向读取。

完整知识的读取由入口 KAT Skill 编排，而不是由 CLI 把 Markdown 正文序列化进 KAT Response。`knowledge.md`、按需 schema 文档和 Query Asset 位于所选 Source Knowledge Package 并随其版本化；Skill 只携带“在选定数据源后读取相应文件”的路由规则，不把外部业务知识复制进中央 `kat/skill/references`。
