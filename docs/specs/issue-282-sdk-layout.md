# KAT SDK 目录与候选版

关联：[Issue #282](https://github.com/maokelong/kat-cli/issues/282)。

## 目标与最小切片

将平台与 Bundled PACK 收入统一版本的 SDK，让用户清楚区分可整体替换的发行内容与独立维护的 External PACK。源码使用 `kat/sdk/platform` 与 `kat/sdk/packs`；部署使用 `kat/sdk/platform/<target>` 与 `kat/sdk/packs`，Skills 源码仍在 `kat/skills`。

SDK 内部 CLI、相邻私有 Python Host、两个 wheel 与 Bundled PACK 同版本。四个 Skills 继续同版本配套交付，使用现有集合归档与校验和生成候选版。用户升级时停止相关任务、成套替换发行目录，不合并新旧文件；External PACK 位于 Data Home 的 `packs` 或显式 `--pack-dir` 指向的 SDK 外目录，升级不覆盖其内容。

## 实现范围

迁移两个源码目录，同步 workspace、构建脚本、CI、测试 fixture 和有效使用文档。调整装配目标路径和 CLI 自身位置验证/Bundled PACK 定位，不改变相邻 Python Host 选择、PACK 重名规则或业务语义。新 ADR 显式记录旧路径合同的替代关系。

## 非目标

不拆仓库，不实现 Issue #280 的独立 Python SDK，不独立发布内部 crate/wheel，不增加自动更新器或兼容版本求解，不引入额外官方分析能力，不迁移用户数据。

## 验收

- Cargo 元数据、格式与相关测试在新路径通过；构建/装配/Skills 合同测试通过。
- 实际候选包包含 SDK 新布局，不遗留旧载荷目录；移动部署目录后仍能启动相邻 Python，完成 inspect、test、run、query。
- 官方与 External PACK 仍按相同规则发现，旧错误布局拒绝；SDK 外 PACK 与用户数据保留。
- 版本元数据一致，归档 SHA-256 可核验，验证记录区分本机和 CI 实际覆盖的平台。
