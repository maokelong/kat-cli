---
status: accepted
---

# Skill 本地 Configuration 选择 KAT Data Home

## 交付边界与验证

**要解决的问题**：用户需要显式选择 KAT 管理数据的根目录，而不必在每次后续命令中重复传递目录。

**最小交付**：只增加 `kat config set/get data-home`，以 Skill 根目录 `config.json` 保存一个有效的 Data Home；已有命令读取此选择。发行包在该位置提供默认的 `{}`。

**不做**：不增加 `--data-home`、环境变量、Profile、通用配置、数据迁移或旧目录的移动、合并和回退读取。

**验证**：CLI 集成测试覆盖 `set` 创建并持久化规范路径、`get` 返回该路径、未显式 `--dataset` 的 Import 使用此根目录、损坏或相对路径配置被拒绝且 `set` 能恢复；整包测试目标以 `cargo test --locked -p kat-cli --no-run` 编译。

本决策取代 ADR-0002 中 KAT Data Home 必须由平台标准目录唯一决定、运行时不修改 Skill、且不提供配置覆盖的部分；ADR-0002 的原子发布与其余载荷边界继续有效。发行包在每个 KAT Skill 安装根目录提供默认空 `config.json`；运行时可改写它，Skill 升级时由新发行包替换，不保留旧值。配置保存当前选择的 KAT Data Home；它是安装本地 Configuration，而不是 KAT Data Home、PACK 或 Dataset。

`config.json` 只接受 `{}` 或 `{"data_home":"<canonical absolute path>"}`：前者表示尚未设置并回落到原有平台默认 Data Home。`kat config set data-home <directory>` 自动创建目标目录，取得 canonical absolute path 后以第二种形状重建 `config.json`；`kat config get data-home` 返回当前生效的 canonical path。Config 缺席时，两个命令和其他 KAT 命令均使用原有平台默认 Data Home，且不主动创建 Config；Config 损坏或字段无效时，除 `set` 可以重建它外，所有形成的操作都失败。所有后续命令使用已设置的 Data Home 管理默认 Dataset、External PACK、Run、Operation log 与 PACK Test Report；显式 `--dataset` 与 `--pack-dir` 仍保持原路径。第一版不提供 `--data-home`、`KAT_DATA_HOME`、多 Profile、通用配置项、配置迁移或旧 Data Home 的自动移动、合并和回退读取；切换只更新当前指针，旧目录保持不动。
