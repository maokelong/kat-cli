---
status: accepted
---

# Ad Hoc Query 通过 Workflow 执行

分析期由 AI 生成或用户提供的 Ad Hoc Query 仍由显式 Workflow 调用 PACK 私有或平台公共查询能力执行，并把结构化结果发布为正常 Run Output；KAT 不新增绕过 Workflow 的远程 `kat sql`，也不把本地 `kat query` 扩展为远程数据库入口。这样 Fixed SQL File 与临时 SQL 可以共享连接、执行和结果处理能力，同时继续使用 Workflow Interface、Run 发布和后续 Output Query 这一条产品链路。
