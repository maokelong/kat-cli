---
status: accepted
---

# Workflow Runtime 独占 Workflow 执行面

KAT 将 Dataset 之后的 SQL、DataFrame、PACK Workflow 与 Run Output 写出统一置于短命 Workflow Runtime，DataFusion 只存在于这一 Query Engine 边界；CLI 仅负责产品入口、进程与日志、候选执行和 Run 发布，Datasource 仅负责领域解码与 Dataset Storage，以避免两套执行面在版本、注册和错误语义上漂移。CLI 与 Runtime 只通过 JSON 文件交换控制事实、通过 Parquet 交换表数据，不跨进程传递 DataFrame、Logical Plan 或内存 Arrow buffer。只有 CLI 验证 Runtime、进程和 Operation log 并持久发布唯一 `manifest.json` 后，候选执行才成为 Run；失败残留和位于 pytest 临时目录的 PACK test 执行都不是可查询 Run。
