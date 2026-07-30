---
status: accepted
---

# Dataset Storage 隐藏物理写入

Data Import 的全部 Dataset 持久写入、解析与 inspection 由 `kat-datasource` 内部的 Dataset Storage Module 独占；Datasource、Workflow 和 CLI 只经窄 Interface 提交逻辑表或消费解析结果，物理目录、Parquet 命名、marker、表名与顶层字段名校验及完整性规则都不成为 SDK，也不建立 catalog、独立 Dataset ID 或第二份元数据。

Dataset 以 `.kat-dataset` 和 `tables/<name>.parquet` 为唯一持久数据面，其他条目不属于 Dataset；解析只接受可规范化的目录和合法受管理普通文件，空 Dataset 合法，受管理文件损坏则失败，Runtime 只注册已解析表而不重新扫描或推断。

不存在或为空的目标可直接初始化，非空目标只有显式 `--overwrite-dataset` 才授权永久清除解析后位置的全部内容（包括未识别或挂载内容），并采用无备份、回滚或恢复的破坏式 fail-fast；重复数据语义仍属 Datasource，第一版不支持 Dataset extension 或通用去重。
