---
status: accepted
---

# 分析流自动选择 PACK 与 Workflow

> ADR-0062 已取消必须先完成 Data Import 或 Source Resolution 才能选择 Workflow 的前置流程，并删除按 Dataset tables/Required tables 筛选候选的规则；Skill 依据问题、Source Guide 与 Workflow Interface 选择 Workflow。本文其余自动选择原则继续有效。

KAT analysis flow 的正常输入是 source 和用户要回答的问题，不要求用户预先理解或指定 PACK 和 Workflow。Data Import 得到 Dataset 后，Skill 先调用 `kat inspect --dataset`，取得由 Dataset Storage 解释的 canonical path、实际 table names 与 Schema；再调用无目标 `kat inspect`，用公开 PACK list 中的 name、title 与 description 缩小候选；最后以 `kat inspect --pack` 逐个展开少量候选 PACK，根据 Workflow 的用途、参数和 Required tables 完成匹配。`required_tables` 不是 Dataset 实际表集合子集的 Workflow 不进入可执行候选；成功 inspection 返回空 `workflows` 时自然不产生候选，不被改写为 KAT 操作失败。Skill 不自行扫描 `pack.toml`、Dataset 文件树或导入 PACK Python。

候选唯一且明确时，Skill 直接执行；只有多个候选会导致实质不同的分析方向时才询问用户。用户仍可以显式绑定 PACK name 和 Workflow 作为高级覆盖，但它不是主产品路径。
