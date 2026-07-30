---
status: accepted
---

# Workflow 只派生数据，不修改 Dataset

Workflow 将获准的 Dataset 表作为不可变 Source，只能派生 DataFrame 并通过返回值发布 Run Output，不能增加、更新、删除或替换 Source；Dataset 持久写入只属于用户显式发起的 Data Import，避免分析策略暗改后续事实输入。

`ctx.sql` 把单语句解析、计划和只读策略交给 DataFusion，禁止 DDL、DML、COPY 与 session mutation；参数只绑定标量值，不接受字符串替换、标识符、SQL 片段或隐式临时 View，因此 KAT 无需维护第二套 SQL parser。
