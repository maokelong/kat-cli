---
status: accepted
---

# Run 通过路径引用 Dataset 当前内容

Dataset 的 canonical 绝对 Unicode 目录路径是唯一身份，不另设 Dataset ID；相对路径以调用进程 cwd 为基准，合法链接解析到目标位置，第一版只承诺本地目录而不为特殊位置增加协议。Run 可在唯一 `manifest.json` 中记录这一可选路径，Output Query 每次读取该位置当前可用的 Dataset，同时保持历史 `output.*` 不变；未提供或当前不可用的 Dataset 不阻断纯 Output 查询，移动或重命名也不会让旧 Run 自动跟随。整体替换同一路径可能使当前 Dataset 与旧 Output 的时钟语义失去一致性，KAT 不保存 snapshot、revision、hash 或 lineage，也不警告或阻止这种比较，跨版本一致性由调用方负责。
