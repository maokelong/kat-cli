---
status: accepted
---

# Hitrace 区分未知扩展与损坏数据

合法但未注册的 Hitrace plugin 或 section type 属于未知扩展：导入继续处理已支持内容，并在成功结果中以稳定、去重的集合显式报告未知项，让 Skill 决定是否继续；出现次数、位置和解码细节只进入 Operation log，不持久化或扩张成通用 warning 体系。

容器、framing 或已注册 plugin 无法解码则属于损坏数据，整个导入失败且不发布部分 Dataset；未知能力不是损坏，已承诺能力也不采用 best-effort。
