---
status: accepted
---

# Run 发布要求可移植的 Output name

本决定部分取代 ADR-0008 与 ADR-0032 中 Output name 只需满足 ASCII snake_case 的决定；ADR-0010 与 ADR-0037 关于 Runtime 数据面所有权和 CLI 不派生或预检候选文件的决定继续有效。`output_name` 同时是 `(run_id, output_name)` 公开身份和 Parquet 文件 stem，因此除 ASCII snake_case 外还排除 Windows 设备保留名，并在 Pack Authoring API、Runtime 与 CLI Response acceptance 中一致执行，不转换或维护第二套物理名称。Runtime 的完整 success Response 是 Output 已物化的权威证明，CLI 只在进程成功退出后严格验收并发布 Manifest，不重复解析或核对候选文件；任何 Runtime、协议或发布失败都不产生 Run，candidate 标识与物理路径保持私有。
