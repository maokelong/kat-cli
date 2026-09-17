---
status: accepted
---

# SDK 统一拥有平台与 Bundled PACK

KAT SDK 是平台运行能力、Pack Authoring API 与 Bundled PACK 的同版本交付集合，用户自己的 External PACK 和数据独立存放。将它们集中到源码 `kat/sdk/{platform,packs}` 和部署 `kat/sdk/{platform/<target>,packs}`，使升级的替换范围明确；平台内部 CLI、私有 Python Host 与 wheels 继续成套更新，不增加独立版本兼容矩阵或拆仓发布。

本决策替代 ADR-0012 的对应源码与部署路径、ADR-0011 的载荷定位路径以及 ADR-0003 的 Bundled PACK 位置，并将 ADR-0002 的平台和官方 PACK 明确命名为 SDK。四个 Skills 仍与 SDK 同版本配套发布，部署位于 `kat` Skill 下；CLI 仍验证相邻部署的 `SKILL.md` 标记并只启动相邻私有 Python。源码、Python import namespace 与部署视图继续分离。

用户停止使用旧部署后整体替换发行目录，不合并旧文件；External PACK 使用 SDK 外的 Data Home `packs` 或显式目录，SDK 发布不包含也不管理用户代码与数据。当前预发布阶段不承诺跨版本 PACK 兼容。独立升级 Skills、公共 Rust SDK、单独分发内部 wheel 与自动更新器均不属于本切片。
