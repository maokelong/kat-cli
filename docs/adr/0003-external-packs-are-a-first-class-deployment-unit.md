---
status: accepted
---

# 外部 PACK 是一等部署单位

KAT 从第一版就支持用户在私有环境中编写和部署不随 KAT Skill 发布的 External PACK，因为领域策略的外部扩展性是产品影响力的核心，不是之后才补的插件能力。External PACK 只分发源码和领域资源，Python 依赖闭包由 KAT Skill 统一提供，以保持私环境中的可移植部署。其测试代码在需要交付时直接位于同一个 PACK 目录，Test Dataset 如有则位于 `tests/datasets/`；二者都与 Workflow、helper 和 manifest 一起版本化；KAT 不拆分测试包或另做版本配对。纯运行部署可以省略 `tests/`，这不使 PACK 的生产 Interface 非法，但显式 `kat test` 必须失败。Bundled PACK 位于 KAT Skill 的 `assets/packs/`、随 Skill 同版本发布，但与 External PACK 使用完全相同的 Pack Authoring API，Workflow Runtime 不向其开放特权。Bundled/External 只描述 KAT 内部的发布与目录来源，不是公共 PACK kind；CLI、inspect 结果和 Runtime 权限都只呈现同一种 PACK。两类 PACK 使用同样的一级 PACK name、同一套 name 校验，并处于同一个扁平 PACK name 作用域。新建的 KAT 自带 PACK 推荐使用 `kat-` 前缀强化品牌，但 KAT 不保留或校验该前缀：其他 PACK 可以使用它，随 Skill 发布的 PACK 也可以省略它，既有 PACK 被收编时不要求改名。名称不证明所有权、发布来源或运行权限。不同 canonical PACK directories 提供相同 name 时 KAT 直接拒绝，不建立隐式覆盖顺序。为避免尚未发布就被兼容性束缚，发布前的 PACK authoring constraints 可以随 KAT 破坏式演进；当前优先验证扩展模型，不承诺跨版本兼容。

KAT 不维护持久 registry。只有无目标 `kat inspect`、`kat inspect --pack`、`kat run` 和 `kat test` 需要列出或选择 PACK，因此它们从 KAT Skill 内的 `assets/packs/`、KAT Data Home 内的 `packs/` 两个固定 search directories，以及可重复的 `--pack-dir <directory>` 执行短命 PACK discovery，得到只在当前进程中使用的 Discovered PACKs；`kat import`、`kat inspect --dataset` 与 `kat query` 不执行 discovery。两个默认 search directories 只扫描直接子目录中的 `pack.toml`；缺席时视为空且不创建，存在却不是可读目录时失败。每个显式目录精确表示一个 PACK directory，必须存在、可读、可 canonicalize，并直接包含 `pack.toml`；KAT 不扫描它的子目录，也不自动判断它是 PACK 还是 PACK 集合。PACK name 只取自 manifest，不从 candidate 目录名推导。全部 candidate 按 canonical PACK directory 去重后共同校验 manifest、统一 PACK name 语法与重名；同一 canonical directory 无论由哪个查找位置发现都只处理一次，重复输入是幂等的，不产生来源身份冲突或覆盖优先级。这使合作伙伴仓库根可以直接成为 PACK directory，又不把可重建的发现结果提升为需要迁移和修复的系统状态，也不让损坏 PACK 阻断无关的 Dataset 操作。公共错误只陈述目录、manifest、name 语法或重名等可修复事实，不要求用户判断 PACK 的内部发布分类。

第一版每个 PACK 都是自包含的发布与执行单位，不能依赖、导入或调用其他 PACK。Python 第三方库属于 Bundled Python Host 的固定依赖集，PACK discovery 不解析或求解依赖。
