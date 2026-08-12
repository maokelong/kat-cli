# Rust / GitHub Actions 编译缓存实践与 Issue #198 取舍

> 调研日期：2026-08-11
>
> 结论先行：成熟项目没有单一的“标准 Rust CI 缓存方案”。抽样项目主要分成两类：uv、Ruff 使用经过清理的 Cargo `target` 缓存；rust-lang/rust 使用远端 sccache；rust-analyzer 当前则注释掉了 Cargo 缓存。kat-cli 最终选择保留 sccache，并把 Cargo 输出移到 Builder 私有、按平台隔离的稳定目录；最终 Payload 仍在随机临时目录中事务性装配。该方案必须用同一 commit 的 warm runs 证明收益，缓存也不能成为正确性依赖。
>
> 本文只使用工具/平台官方文档和源码，以及成熟项目当前仓库文件。项目样本用于证明可行做法与差异，不是统计学意义上的“行业标准”。性能阈值属于 kat-cli 的工程取舍，不伪装成上游保证。

> 设计会话更新：结合 GitHub-hosted runner 的 job 级新 VM 边界，最终方案不再把 Cargo `target` 放进每次随机临时根。Builder 改用仓库内按平台隔离的稳定私有 Cargo cache，同时继续保留最终 Payload 的同目录 staging、完整验证和一次 rename 提交。该选择仍须通过一轮 cold baseline、一次 fill 和两轮 warm 门禁证明价值；失败则整体撤回本次缓存切片。

## 1. 调研范围

本文回答六个问题：

1. 随机或隔离 `target-dir` 时，怎样避免绝对路径和环境变量污染 sccache 的 Rust key；
2. 应缓存完整 Cargo `target`，还是只缓存编译器输出；
3. 安装或远端缓存故障怎样 fail-open，是否自动冷构建重试；
4. 统计应使用 action 的 post 阶段，还是手工 `sccache --show-stats`；
5. 怎样验收性能收益并设置止损线；
6. cache retention 和容量怎样管理。

相关 kat-cli 约束来自 Issue #198 / PR #199：Builder 把 Cargo 输出放在每次新建的临时目录；缓存只能是可选加速；不能改变 Payload 内容、profile 或 feature；不引入完整 `target` 缓存；当前双平台复跑的 Rust hit 均为 0，缓存约占 1.12 GiB。

## 2. Rust cache key 与随机 `target-dir`

### 2.1 上游事实

Cargo 支持用 `CARGO_TARGET_DIR`、`CARGO_BUILD_TARGET_DIR` / `build.target-dir` 或 `cargo build --target-dir` 选择输出目录；命令行参数可以覆盖配置。[Cargo build cache](https://doc.rust-lang.org/stable/cargo/reference/build-cache.html)、[Cargo configuration](https://doc.rust-lang.org/cargo/reference/config.html#buildtarget-dir)

sccache v0.16.0 的 RustHasher 对 rustc 参数和环境的处理并不对称：

- 生成 key 时明确排除 `--out-dir`、`-L` 和 `--extern`，因为这些路径本身不影响输出，被引用的 rlib/staticlib 内容会单独参与哈希；上游测试也断言这些路径改变后 key 仍相等。[RustHasher key construction](https://github.com/mozilla/sccache/blob/v0.16.0/src/compiler/rust.rs#L1385-L1423)、[ignored-path test](https://github.com/mozilla/sccache/blob/v0.16.0/src/compiler/rust.rs#L3548-L3598)
- 它会哈希 rustc dep-info 声明的环境依赖，并额外哈希几乎所有以 `CARGO_` 开头的 rustc 环境变量；只硬编码排除 `CARGO_MAKEFLAGS`、`CARGO_REGISTRIES_*`、`CARGO_BUILD_JOBS` 和 `CARGO_ENCODED_RUSTFLAGS`。因此随机 `CARGO_TARGET_DIR` 会直接改变所有可缓存 Rust invocation 的 key。[RustHasher environment hashing](https://github.com/mozilla/sccache/blob/v0.16.0/src/compiler/rust.rs#L1436-L1477)
- 当前工作目录 `cwd` 也参与 key。源码 checkout 路径必须跨运行稳定，单独修 target 路径不能补救随机源码根。[RustHasher cwd hashing](https://github.com/mozilla/sccache/blob/v0.16.0/src/compiler/rust.rs#L1470-L1477)
- sccache 官方文档把 `SCCACHE_BASEDIRS` 描述为通用的绝对路径规范化手段，但 v0.16.0 的 RustHasher 不接收或读取 `basedirs`；该版本仓库中的实际调用位于 C/C++ 预处理路径。因此不能把 `SCCACHE_BASEDIRS` 当作当前 Rust key 的修复。[configuration docs](https://github.com/mozilla/sccache/blob/v0.16.0/docs/Configuration.md#base-directories-to-strip-from-source-paths-during-cache-key-generation)、[RustHasher source](https://github.com/mozilla/sccache/blob/v0.16.0/src/compiler/rust.rs#L112-L130)

### 2.2 常见做法、适用条件、与 kat-cli 的差异

抽样 workflow 通常直接在固定 checkout 的默认 `target` 下构建，而不是每次随机化输出根。uv 的开发二进制 workflow 即使把 Windows checkout 搬到 Dev Drive，也给缓存 action 和 Cargo 使用同一个显式 workspace；Ruff 同样使用稳定 workspace 和按 job/profile 区分的 target cache。[uv build-dev workflow](https://github.com/astral-sh/uv/blob/e39e0b21b30ada262bedd577e115bd8c90630862/.github/workflows/build-dev-binaries.yml#L251-L268)、[Ruff CI](https://github.com/astral-sh/ruff/blob/d08b174e09a23c0a0413b7e7db7dc67d69593eac/.github/workflows/ci.yaml#L320-L340)

最初的最小候选是继续使用随机 Cargo 目录，仅改变传参方式：

1. 保留随机临时目录和现有产物查找/清理；
2. 用 `cargo build --target-dir <随机目录>/cargo-target` 传递输出位置；
3. 从 Cargo 子进程环境中移除继承的 `CARGO_TARGET_DIR` 和 `CARGO_BUILD_TARGET_DIR`；
4. 保持 Cargo 的工作目录为稳定 checkout；
5. 接受读取随机 `OUT_DIR` 等路径的 build script 或 crate 仍可能 miss。

这能移除环境变量本身的随机性，却不能保证所有随机派生路径都不进入 rustc 环境、参数或 dep-info。结合第 8 节的 runner 生命周期分析，最终方案改为稳定的 Builder 私有平台目录 `target/kat/cargo/<platform>`，同时保留独立的随机 Payload staging。它是由 v0.16.0 key 规则推导出的 **kat-cli 专用实验**，不是上游文档给出的通用 recipe，其价值必须由真实 warm run 验证。

## 3. 完整 Cargo `target` 与 sccache

### 3.1 两类方案解决的问题不同

GitHub 官方把依赖和昂贵的中间产物都视为 cache 的合理对象，但同时要求 job 在 cache 不存在时仍能重新生成它们。[GitHub dependency caching](https://docs.github.com/en/actions/concepts/workflows-and-actions/dependency-caching)

`Swatinem/rust-cache` 代表“缓存 Cargo 状态”路线。它默认缓存 `~/.cargo` 和 workspace `target` 中的依赖构建产物；保存前删除未使用依赖、workspace 自身产物、incremental 产物和一周以上的旧构建产物，并默认设置 `CARGO_INCREMENTAL=0`。它还允许只在 main 保存、其他 job 只恢复。[rust-cache cache details](https://github.com/Swatinem/rust-cache/blob/a45951ff880207c249adf57334cf2e9bd81d6e1e/README.md#cache-details)

sccache 代表“缓存 compiler invocation 输出”路线。它通过 `RUSTC_WRAPPER` 包住 rustc，能使用本地或远端 backend；Rust 的 `bin`、`dylib`、`cdylib`、`proc-macro` 和 incremental compilation 等 invocation 不可缓存。[sccache Rust caveats](https://github.com/mozilla/sccache/blob/v0.16.0/docs/Rust.md)、[sccache README](https://github.com/mozilla/sccache/tree/v0.16.0#known-caveats)

完整 target cache 能覆盖 Cargo fingerprint、build script/proc-macro 结果等 sccache 不能覆盖的工作，但体积更大，并把路径、mtime、profile、features 和共享状态管理带进恢复边界。sccache 粒度更窄、隔离性更好，但最终收益取决于可缓存 compilation 的占比和 key 稳定性。

### 3.2 成熟项目样本

- uv 的开发构建和 Ruff 的 CI 使用 `Swatinem/rust-cache`。Ruff 只在 `main` 保存，PR 主要恢复；不同消费者通过 `shared-key` 复用同一 debug cache。uv 还明确启用 workspace crate cache。这说明维护良好的 target cache 是常见且成熟的选择，但它们的 workspace/target 路径稳定，与 kat-cli 的临时 Builder 不同。[uv workflow](https://github.com/astral-sh/uv/blob/e39e0b21b30ada262bedd577e115bd8c90630862/.github/workflows/build-dev-binaries.yml#L32-L42)、[Ruff workflow](https://github.com/astral-sh/ruff/blob/d08b174e09a23c0a0413b7e7db7dc67d69593eac/.github/workflows/ci.yaml#L292-L300)
- uv 的多平台 release binary workflow 没有同样接入 rust-cache，而是直接构建并上传产物。这证明“普通 CI 缓存 target”不自动意味着“每个发布组包路径也缓存 target”。[uv release build](https://github.com/astral-sh/uv/blob/e39e0b21b30ada262bedd577e115bd8c90630862/.github/workflows/build-release-binaries.yml)
- rust-lang/rust 使用项目自有 S3 bucket 的 sccache，并通过专用脚本安装，属于规模更大、基础设施更重的远端 compiler cache；不能直接类比为小型仓库应自建 backend。[rust CI](https://github.com/rust-lang/rust/blob/0913b18e489ac1011b580e31fa5559654be12bfc/.github/workflows/ci.yml#L85-L95)
- rust-analyzer 当前 workflow 中的 `Swatinem/rust-cache` 步骤被注释掉，说明“不缓存”也是实际维护选择；仓库文件没有给出可泛化的原因，本文不替维护者猜测。[rust-analyzer CI](https://github.com/rust-lang/rust-analyzer/blob/57bea800b866168ac4f310333e326b92ccba7aca/.github/workflows/ci.yaml#L108-L114)

kat-cli 没有采用 Actions cache 保存和恢复完整 Cargo `target`。稳定的仓库内目录只在当前 runner/job 或本地调用期间供 Cargo 使用，跨 GitHub-hosted jobs 的复用仍仅来自 sccache；因此没有新增另一套远端 target cache，也没有把 Cargo 目录提升为可靠状态。该边界与第 9 节合同一致。

## 4. Fail-open 与统计

### 4.1 故障处理

sccache 默认在无法与本地 server 通信时使构建失败；官方提供 `SCCACHE_IGNORE_SERVER_IO_ERROR=1`，让这类 server I/O 错误回退到直接调用 compiler。这个变量不是“所有下载、配置、backend 和编译错误都忽略”的总开关。[sccache failure behavior](https://github.com/mozilla/sccache/tree/v0.16.0#usage)

GitHub cache 的平台契约同样是加速可缺失：cache miss 时应重新生成；低信任 workflow 无法保存时会 warning 而不使 job 失败。[GitHub cache behavior and low-trust access](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching#cache-access-for-low-trust-workflow-triggers)

抽样 workflow 一般没有“Cargo 失败后自动把整次长构建再跑一遍”的逻辑。rust-lang/rust 的生产 CI 将 `sccache --start-server` 和结束统计写成 `|| true`，而不是失败后重跑完整编译；PR 没有 AWS 写凭证时改为匿名只读 S3 cache。[rust CI startup/stats](https://github.com/rust-lang/rust/blob/0913b18e489ac1011b580e31fa5559654be12bfc/src/ci/run.sh#L241-L245)、[PR cache credentials](https://github.com/rust-lang/rust/blob/0913b18e489ac1011b580e31fa5559654be12bfc/src/ci/docker/run.sh#L239-L250) 自动重跑无法可靠区分 cache wrapper 故障和真正的编译错误，还可能把门禁时间翻倍。对 kat-cli 更稳妥的 fail-open 边界是：

- action 安装失败：不设置 `RUSTC_WRAPPER`，直接冷构建；
- sccache server I/O：依赖官方 `SCCACHE_IGNORE_SERVER_IO_ERROR=1`；
- 其他未知失败：保留显式 `cold-build` 重跑入口，不自动重跑 Cargo。

这是对错误类别的显式分层。PR 文案应避免承诺“任何缓存错误都不会影响构建”。

### 4.2 统计

`mozilla-actions/sccache-action@v0.0.11` 自带 post action。post 源码同时执行 human-readable 和 JSON `--show-stats`，并写入 log、notice 和 Job Summary；`disable_annotations` 会关闭该报告。[action metadata](https://github.com/mozilla-actions/sccache-action/blob/fc920bf0ec8de6ee65d409111f7ec508035751ba/action.yml#L14-L20)、[post implementation](https://github.com/mozilla-actions/sccache-action/blob/fc920bf0ec8de6ee65d409111f7ec508035751ba/src/show_stats.ts#L29-L70)

因此常态观测只保留 action post 即可，额外的手工 `sccache --show-stats` 是重复输出。只有必须在 job 中途把 JSON 解析成机器门槛、或 post 被关闭时，才需要自有统计步骤；这种情况下应只保留一套报告责任。

## 5. 性能验收与止损

上游没有“命中率多少”或“加速 20%”的行业标准。cacheability 随 crate 类型、build script、profile、runner 和网络而变；项目 workflow 样本也没有把固定比例写成普遍门禁。因此以下是 **kat-cli 的决策规则**，不是 sccache 保证：

1. 同一 commit、相同 workflow/runner 形态先以 `cold-build=true` 运行一次真实冷基线，再运行 1 次非 cold 填充和 2 次非 cold warm；
2. 每次记录 Linux/Windows 的 Cargo elapsed、Payload job elapsed、Rust hits/misses、cache errors/write errors 和 repository cache size；
3. 两次 warm 的双平台 Rust hits 都必须大于 0，relocated smoke 必须通过；
4. 两次 warm 的双平台 Payload job 都至少比 `cold-build=true` 基线快 20%；
5. 只允许一次上述最小 key 修复实验。任一平台仍无 Rust hit，或任一 warm 未过 20%，撤回 PR #199 的 sccache 机制，不继续叠加共享 target、自动重试、fork sccache 或自建 backend。

两次 warm 不是严谨性能基准，但能以可接受的 CI 成本排除“首次填充”和一次性 runner 波动；独立 cold baseline 则避免旧 PR-scope cache 污染基线。结果必须同时看 hit 与 elapsed：有 hit 不等于关键路径更快，elapsed 变快而 Rust 仍 0 hit 也不能归因于缓存。

sccache 自身的 GHA backend 集成测试采用更窄的正确性验证：连续两次 `cargo clean && cargo build`，第二次直接断言 cache hits 大于 0。这支持“先证明跨 clean 命中”的验收结构，但它不测项目级耗时，也不能替代 kat-cli 的 20% 业务止损线。[sccache integration test](https://github.com/mozilla/sccache/blob/46e96ab443c52bfd796071fca5500efdcfbc89fb/.github/workflows/integration-tests.yml#L99-L115)

## 6. Retention 与容量

GitHub 当前默认删除超过 7 天未访问的 cache；默认仓库总容量是 10 GB，达到上限后按最近最少访问顺序淘汰，频繁写入可能产生 cache thrashing。PR cache 又受 merge ref scope 限制，只能被同一 PR 的 rerun 恢复；默认分支建立的 cache 才能稳定供后续 PR 恢复。[GitHub cache limits and scope](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching#usage-limits-and-eviction-policy)

GitHub 允许仓库配置 retention 和容量上限，但 retention 只是“最长保留时间”，不能阻止容量压力下提前淘汰。[GitHub repository cache settings](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/enabling-features-for-your-repository/managing-github-actions-settings-for-a-repository#configuring-cache-settings-for-your-repository)、[cache REST API](https://docs.github.com/en/rest/actions/cache#get-github-actions-cache-retention-limit-for-a-repository)

所以 retention 不应凭感觉设得越长越好：

- 若主分支或同一 PR 至少每周会触达 cache，默认 7 天已经会随访问刷新；
- 只有实际复用间隔常超过 7 天，并且容量没有 thrashing 时，21 天才可能增加命中；
- 以本次约 1.12 GiB 的一次双平台填充估算，默认 10 GB 只能容纳有限代际；应先证明 warm 收益，再决定是否值得占用更长 retention；
- 持续查看 repository cache usage/count 和最老/最近访问记录；出现接近容量上限、频繁淘汰或命中率下降时，优先缩短 retention/减少写入 scope，而不是付费扩容。

无论本次最小实验成功或失败，仓库 retention 都先恢复为变更前的 7 天。只有后续实际监测证明复用间隔经常超过 7 天、容量仍有余量且没有 cache thrashing，才另行评估 21 天；它始终只是仓库运营参数，不是代码正确性契约。

## 7. 对 Q6–Q8 的调查建议与最终选择

综合证据，建议接受此前三个方向，但收紧表述：

- **Q6：调查阶段的最小候选**是保留随机 Cargo 目录，仅改用 `--target-dir` 并清除继承的两个 target-dir 环境变量。进一步核对 runner 生命周期后，设计会话选择了覆盖更多随机派生路径的稳定 Builder 私有 Cargo cache；第 8、9 节记录其边界。
- **Q7：只保留 action post 统计。** 删除重复的手工 human-readable stats；验收直接保存 post 的 human/JSON 结果。若未来要自动判阈值，再新增单一 JSON 门禁而不是两套报告。
- **Q8：不自动冷构建重试。** 安装失败通过“不启用 wrapper”降级，server I/O 使用官方 ignore 选项，其他错误由显式 `cold-build` 人工重跑；文案只承诺已验证的故障类别。

这套方案仍然不是“已证明优化”：只有第 5 节的 cold baseline + fill + 2 warm 验收通过后，PR #199 才完成业务目标。

## 8. GitHub Runner 隔离与 Builder 事务性装配是两层责任

### 8.1 GitHub 默认隔离和持久化边界

GitHub-hosted runner 已经提供 **job 级**机器隔离。GitHub 官方说明，除 single-CPU runner 是共享 VM 上的容器外，每个 GitHub-hosted runner 都是一个新 VM；其缓存文档又明确说 hosted job 从 clean runner image 启动、每次都要重新下载依赖。因此在当前 `ubuntu-*` / `windows-*` hosted jobs 上，checkout workspace、普通 Cargo `target`、runner 本机 Docker daemon 的 image/layer/volume 都不能作为下一 job 或下一 run 的持久状态。[GitHub-hosted runners](https://docs.github.com/en/actions/concepts/runners/github-hosted-runners)、[Dependency caching](https://docs.github.com/en/actions/concepts/workflows-and-actions/dependency-caching)

`jobs.<job_id>.container` 是这台 runner 内额外的一层 job execution environment：未声明 `container` 时，步骤直接在 `runs-on` 选择的 host 上运行；声明后，同一 job 的普通步骤在 job container 内运行，container actions 作为共享网络和 volume mounts 的 sibling containers。Runner 在 job 开始创建专用网络，并注册 `always()` post-job cleanup；job 结束会删除 containers 和 network。因此 job container 能加强同一 VM 内的执行边界，却不会自动提供跨 job/run 的 Docker layer cache。[Running jobs in a container](https://docs.github.com/en/actions/how-tos/write-workflows/choose-where-workflows-run/run-jobs-in-a-container)、[actions/runner container lifecycle](https://github.com/actions/runner/blob/99e01149303b194e08778cdc9c03fa2703d03fb2/src/Runner.Worker/ContainerOperationProvider.cs#L46-L104)、[post-job removal](https://github.com/actions/runner/blob/99e01149303b194e08778cdc9c03fa2703d03fb2/src/Runner.Worker/ContainerOperationProvider.cs#L144-L164)

self-hosted runner 不具备上述默认保证。GitHub 明确说它“不需要每个 job 都是 clean instance”；workspace、Cargo `target`、Docker layers 或其他本机状态可能偶然留存，但这种留存由 runner 管理者负责，不能当作可移植 workflow 合同。若未来改用 self-hosted runner，应由 runner 镜像/清理策略或显式 cache 设计该生命周期，而不是依赖上一次 job 的残留。[Self-hosted runners](https://docs.github.com/en/actions/concepts/runners/self-hosted-runners)

跨边界持久化必须显式声明：

- **cache** 用于跨 workflow runs 复用可重新生成的依赖或中间产物；miss 时 job 必须仍能重建。即使使用 self-hosted runner，Actions cache 也存放在 GitHub-owned cloud storage。[Dependency caching](https://docs.github.com/en/actions/concepts/workflows-and-actions/dependency-caching)
- **artifact** 用于把 job 产物传给其他 job，或在 workflow 结束后保存二进制、日志等结果；当前 Payload workflow 正是通过 upload/download artifact 把两个平台载荷交给 assemble job。[Store and share data with workflow artifacts](https://docs.github.com/en/actions/tutorials/store-and-share-data)、`.github/workflows/build-linux-payload-ci.yml:60-66`、`.github/workflows/build-windows-payload-ci.yml:78-85`。
- 普通 workspace、Cargo `target` 和 Docker layers 都不是隐式持久化面。需要跨 hosted jobs/runs 复用时，必须显式使用 Actions cache、artifact，或对 container image/layers 使用 registry / 对应的显式 build cache；仅仅增加 job container 不会产生这种复用。

### 8.2 当前 Builder 不是只服务 PR gate

仓库内目前能找到的自动化调用入口是 Linux/Windows Payload reusable workflows：`.github/workflows/build-linux-payload-ci.yml:46-58` 和 `.github/workflows/build-windows-payload-ci.yml:68-76`。但两个 Builder 同时是可在仓库 checkout 中直接执行的 CLI，公开了 `--repository`、`--output`、预取资源、offline、Cargo 等参数和本地默认路径：`build/build_linux_payload.py:183-242`、`build/build_windows_payload.py:492-550`。它们不是 KAT 对终端用户承诺兼容的独立产品 API，却是 repository-local build tool，而不只是写死在 PR gate 中的脚本。

更重要的是，ADR-0002 把相同 Platform Payload Builder 指定为 Release 的平台 local artifact jobs，再由 Skill Assembly Adapter 汇总成唯一发布物（`docs/adr/0002-skill-and-runtime-ship-atomically.md:9-11`）。当前 checkout 中没有另一个已提交的 release workflow 调用，因此不能声称 Release 已经实际接线；但 Builder 的生命周期设计仍必须同时适用于本地调用和 ADR 已确定的 Release 复用，不能把 hosted runner 的一次性 VM 当成 Builder API 的前置条件。

### 8.3 外层 `TemporaryDirectory` 实际承担的职责

`build/payload_builder.py:898-927` 在最终 `output.parent` 下创建临时根，并在其中完成四类工作：

1. 在 `payload/` staging 中安装私有 Python、locked requirements、Workflow wheel 和最终 CLI；
2. 保存解压中的 Python/uv/VC Runtime 等中间目录和多个 uv 临时 cache，避免它们进入最终 Payload（`build/payload_builder.py:770-814`）；
3. 修改前还把 Cargo 输出放在随机 `cargo-target/`；本次方案把它移到稳定的平台私有 cache；
4. 全部 finalize 和 shape validation 成功后才用 `stage.replace(output)` 发布；异常或成功退出 context 时清理其余临时内容。

其中第 4 项不是 Runner 隔离能替代的。临时根刻意位于 `output.parent`，使 staging 与 output 位于同一文件系统；`replace` 才能作为单写入者流程的提交点，避免本地调用、Release job 或同一 job 后续步骤看见半成品。输出已存在时 Builder 还会在动手前拒绝，而不是合并或删除旧输出（`build/payload_builder.py:862-881`）。这与 Skill Assembly 自身的同目录 staging、失败清理和 rename 规则一致（`build/assemble_skill.py:83-123`），也是 ADR-0002 明确要求的两阶段原子发布语义。

### 8.4 可以删什么，必须保留什么

因此不能从“hosted runner 是 fresh VM”推出“删除整个 Builder 临时目录机制”。正确拆分是：

- **可以移出随机临时根：Cargo `target` 的随机隔离。** 对当前 hosted job，job VM 已隔离不同 job/run；对本地和 Release 调用，Cargo 自己管理一个调用方可见、稳定的 build cache 也不影响最终 Payload 的事务性发布。将 Cargo target 设为稳定路径，或继续使用随机 `--target-dir` 但消除 key 污染，是独立的性能选择，不应与 Payload staging 绑定。
- **可以按需简化：只为下载/解压/uv 安装服务的临时 cache。** 它们不需要跨 hosted jobs 保留；要跨 run 加速必须显式放进 Actions cache。是否继续留在临时根，取决于本地失败清理和“不把构建垃圾混入 Payload”的便利性，而不是隔离正确性。
- **必须保留：最终 Payload 的同目录 staging、完整验证、失败清理和一次 rename 提交。** 这保护的是调用者可见输出的事务性，不是不同 GitHub jobs 之间的机器隔离；本地 CLI 和 Release 复用同样需要。
- **必须保持 cache 非正确性依赖。** 不论最终选择稳定 Cargo target、sccache 或不缓存，cache miss/淘汰都只能影响耗时，不能改变产物和门禁结果。

对 Issue #198 的直接含义是：可以重新评估“随机 Cargo target”是否还有必要，但不应借此删除 `payload/` staging 或 `stage.replace(output)`。最小设计应把 **Runner/job 隔离**、**可选编译缓存**、**Payload 事务性发布** 三个生命周期分开。

## 9. 已确认的 Issue #198 最小交付合同

设计会话最终确认以下边界：

1. Cargo cache 是 Builder 私有、按平台隔离、可整体删除的重建缓存：`target/kat/cargo/linux-x86_64/` 与 `target/kat/cargo/windows-x86_64/`。不新增公共 CLI 配置，也不把该目录解释为系统状态。
2. Builder 使用 `cargo build --target-dir <platform-cache>`，并从 Cargo 子进程环境移除 `CARGO_TARGET_DIR` 与 `CARGO_BUILD_TARGET_DIR`，避免调用环境重新污染 sccache key。
3. `--output` 与内部 Cargo cache 任一方向重叠时，在构建开始前拒绝；Builder 成功或失败后均不主动删除 Cargo cache，失效与重建交给 Cargo，调用者可以随时整体删除。
4. 最终 Payload 继续在 `output.parent` 下的随机 staging 中装配。只有所有 finalize 与 shape validation 完成后，才以一次 rename 发布；异常时 staging 由 Builder 清理。
5. 删除重复的手工 `sccache --show-stats`，只保留固定版本 Mozilla Action 的 post report。安装失败时不启用 wrapper；官方支持的 server I/O 路径按 sccache 语义回退；其他失败由显式 `cold-build` 处理，不自动重跑完整 Cargo。
6. 仓库 Actions cache retention 恢复为 7 天。只有实际监测证明复用间隔经常超过 7 天且没有容量 thrashing，才另行调整。
7. 新实现提交后、性能验收开始前，删除范围严格限定为 `refs/pull/199/merge` 的旧 Actions cache。该批 1326 个、约 1.12 GiB 的条目来自随机 Cargo target key；删除不触及源码、artifact、Release 或其他 ref cache，并使后续 fill/warm 证据不受旧条目污染。
8. 验收使用同一 commit 和同一 workflow 形态连续运行：一次 `cold-build=true` 基线、一次非 cold 填充、两次非 cold warm。两次 warm 的 Linux/Windows Rust hits 都必须大于零，Payload job 都必须比冷基线至少快 20%，且 relocated smoke 全部通过；同时记录 Cargo elapsed、job elapsed、hit/miss、错误和 repository cache size。
9. 只允许这一套最小实验。任一平台或任一 warm 未过验收线，就从 PR #199 撤回 sccache、稳定 Cargo cache 及相关 workflow/test/doc 交付，并保持 7 天 retention；后续调查不得作为未实现价值的合入理由。

本次记录属于 Issue #198 的实现与验收证据，不引入新的 KAT 产品领域词汇，也不满足新增 ADR 所需的难逆、意外和真实长期取舍三个条件。最终运行链接、数据和合入/撤回结论仍应回写 Issue #198 与 PR #199。
