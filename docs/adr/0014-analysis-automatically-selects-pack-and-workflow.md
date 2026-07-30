---
status: accepted
---

# 分析流自动选择 PACK 与 Workflow

KAT analysis flow 的正常输入是 source 和用户问题，不要求预先指定 PACK 或 Workflow；Data Import 后，Skill 依次使用 Dataset inspection、公开 PACK list 和少量目标 PACK inspection，依据用途、参数与 Required tables 选择可执行 Workflow，而不自行扫描 manifest、Dataset 文件树或导入 PACK Python。候选唯一明确时直接执行，多个候选会导致实质不同方向时才询问用户；显式绑定 PACK name 与 Workflow 仍是高级覆盖而非主路径。
