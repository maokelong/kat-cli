---
status: accepted
---

# 单一 KAT Skill 隐藏内部操作动词

KAT 只向用户发布一个 `$kat` Skill，用户以自然语言表达分析或 PACK 开发目标，Skill 再路由到相应 flow；单一 `kat` 可执行文件内部使用 `import`、`inspect`、`test`、`run` 和 `query` 等顶层类型化动词，PACK、Workflow、Dataset 与 Run 只是明确目标，不形成用户要选择的多个 Skills、名词命令树或泛化执行入口。各动词保持窄而明确的职责：Datasource 整体生成或替换 Dataset，inspect 逐步展开 PACK、Workflow 与 Dataset，run 发布 Run，query 有界核验 Output，test 交给 pytest；CLI 不以自动探测、通用参数 DSL、复合选择器或持久 registry 模糊这些边界。分析 flow 在内部串联 Data Import、检查与选择、执行和有界查询，PACK authoring flow 串联发现、校验、测试与诊断，让用户只需理解自己的目标和 KAT，而不是内部状态机。
