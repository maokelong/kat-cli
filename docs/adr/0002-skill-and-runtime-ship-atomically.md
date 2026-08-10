---
status: accepted
---

# Skill 与运行时原子发布

KAT 只把 Skill constraints 作为稳定产品面，并将 Skill 定义、`SKILL.md` 中的平台选择逻辑、KAT CLI、Bundled Python Host 与所支持平台的二进制载荷按同一版本原子发布。这一选择不为底层 CLI 参数承诺独立兼容性，以避免 Skill 与二进制独立升级造成的协议错配，代价是普通终端用户不获得一个单独承诺稳定的 CLI 产品面。

发布采用两阶段构建：`kat/platform/workflow` 先通过成熟 PEP 517 backend 构建一个私有纯 Python Workflow Host wheel，同时包含 PACK 使用的 `import kat` 与 CLI 启动的私有 Runtime module。Linux 与 Windows 的 Platform Payload Builder 在原生平台使用 Cargo 构建 KAT CLI，并以固定版本的 `uv` 获取 `python-build-standalone` 提供的 CPython 3.14 standard-GIL 可重定位目录、把这一个 KAT wheel 与锁文件中的平台第三方预编译 wheels 安装进 Host，生成包含完整标准库、Workflow Runtime、Pack Authoring API、Click、PyArrow、DataFusion 与测试依赖的私有 Python Host。每个 KAT 版本精确锁定 CPython `3.14.z`、PBS release 与 wheel hashes，构建时不浮动选择 latest；不同时携带 CPython 3.13 或 3.14 free-threaded 变体。它不创建 venv，不冻结 Python 应用，不手工复制 site-packages，也不在用户机器解包、下载或解析依赖。一个只理解标准 Skill 路径映射的 Skill Assembly Adapter 再把两个完整载荷、Skill source 和 Bundled PACK 组合为唯一的 `dist/kat`。调用方必须在整个装配期间独占一个预先不存在的输出路径；Adapter 在同目录 staging 中完成装配后 rename 到该路径，入口时已有输出直接拒绝，不合并也不清理已有内容。这里的原子发布只承诺单写入者流程不会暴露部分 deployment view，不提供跨进程 no-replace 或 compare-and-swap；多个 Assembly 竞争同一输出不在本决策范围内。重复构建必须使用新输出路径，或由外层发布生命周期先管理旧产物。该 Adapter 是 deployment view 的唯一写入者，但不替代 Cargo、Python 依赖工具、平台打包器或发布系统，也不在第一版增加版本矩阵、签名、哈希和复杂发布一致性机制。

外层发布生命周期由固定版本的 `dist`（原 cargo-dist）管理，包括版本与 tag、原生目标 runner、local/global artifact jobs、校验和、托管和 GitHub Release。各 Platform Payload Builder 作为 local artifact jobs 运行，Skill Assembly Adapter 作为 global artifact job 汇总两个黑盒载荷；KAT 不手改 `dist` 生成的 release workflow，也不在其上重建发布编排器。`dist` 不理解 Skill anatomy，最终路径映射仍只属于薄 Assembly Adapter。当前固定的 `dist 0.32` 也不能发现自定义 global job 产生的 opaque artifact，因此最终 Skill 与配套 SHA-256 文件由同一个 global artifact job 使用标准归档和哈希工具一次产出；`checksum = "false"` 只阻止 `dist` 为私有原生 payload 规划公开校验文件，不改变 `dist` 对 job 顺序、托管和 Release 的所有权。真实 tag 验证同时证明 `dist-manifest.json` 会继续声明未公开的原生 payload 归档，却无法登记最终 opaque Skill，因此它只作为发布计划中间产物使用；生成流水线通过 `dist` 官方 `post-announce` job 核对 tag、commit、最终 Skill、配套 SHA-256 和完整资产集合，再将该 manifest 从 GitHub Release 删除。最终公开资产严格只有 Skill 与配套 SHA-256 文件。`release/kat/dist.toml` 是 KAT 发布版本入口，发布准备阶段必须校验 Cargo workspace 与 Bundled Python Host 的 package metadata 使用同一版本。后续版本若能声明并校验自定义 global artifact，应删除 global artifact 与 manifest 收尾例外，而不增加第二套发布编排。

本决策评估过 `dist 0.32` 的 package-local [`extra-artifacts`](https://axodotdev.github.io/cargo-dist/book/reference/config.html#extra-artifacts)，但它不能同时满足当前边界。计划级最小复现是在 `release/kat/dist.toml` 临时加入以下配置；`build` 在 plan 和 generate 阶段不会执行，因此不需要保留实验 helper：

```toml
[[dist.extra-artifacts]]
artifacts = [
    "../../target/distrib/release/kat-skill-0.1.0.tar.gz",
    "../../target/distrib/release/kat-skill-0.1.0.tar.gz.sha256",
]
build = ["python", "-c", "raise SystemExit('plan-only probe')"]
```

固定使用 `dist 0.32.0` 运行 `dist plan --output-format=json`，`releases[0].artifacts` 会同时列出上述两个 extra artifacts 与 `kat-x86_64-pc-windows-msvc.zip`、`kat-x86_64-unknown-linux-gnu.tar.xz`；运行 `dist generate` 后，built-in `build-global-artifacts` 与 `custom-payload-ci` 是共同依赖两个 local payload jobs 的并列 job，`host` 再同时等待二者。把 `release/kat/dist.toml` 的 `binaries` 临时改成 `[]` 后重跑，错误为 `This workspace doesn't have anything for dist to Release!`；恢复 `binaries`、把 `dist-workspace.toml` 的 `targets` 临时改成 `[]` 后重跑，错误为 `specified no targets to build!`。这证明不能通过关闭 generic binary/target 计划只保留 extra artifacts，也不能让 custom PR smoke 消费 built-in global job 随后产生的同一个归档。

构建级 probe 让临时 `build` helper 产生上述两个文件后运行 `dist build --artifacts=global --output-format=json`，只证明 extra artifacts 会被登记和复制；它不消除前述完整 plan 与 job 依赖限制。`dist 0.32` 的 [`ExtraArtifact`](https://github.com/axodotdev/cargo-dist/blob/v0.32.0/cargo-dist/src/tasks.rs) 不关联 checksum，配套 SHA-256 仍必须由构建命令自行产生。托管阶段也仍会公开 `dist-manifest.json`，不满足最终 Release 严格只有 Skill 与配套校验文件的资产契约。基于这些约束，当前 custom global job 与 post-announce 收尾是 `dist 0.32` 生命周期内的最小适配，而不是对 extra-artifacts 的重复实现。

第一阶段的完整载荷矩阵覆盖 glibc 2.28 及以上的 Linux x86_64，并把 Windows 10 及以上的 x86_64 客户端（包括 Windows 11）作为预发布候选目标；Windows 正式支持仍以 [Issue #143](https://github.com/maokelong/kat-rs/issues/143) 要求的干净客户端完整验收为准。Linux 下限服从 DataFusion/PyArrow 官方 `manylinux_2_28_x86_64` wheels，不为扩大兼容面自行构建 native wheel；`scripts/targets/linux-x86_64/` 的路径不把 glibc 版本编码成第二层平台身份。Windows 候选下限服从 CPython 与 Rust MSVC target 共同明确支持的客户端范围，不从 `win_amd64` wheel tag 猜测更旧或更细的 OS build 兼容性；Windows 7/8.1 与 Windows Server 第一版均不进入候选矩阵。Windows Platform Payload Builder 根据最终 CPython、KAT CLI 与 native wheels 的实际依赖，从 Microsoft 官方允许再分发的文件集合中确定并随载荷放置 app-local Visual C++ Runtime DLL 闭包，不维护一份脱离产物的固定 DLL 清单。musl、更旧 glibc 和其他不支持的平台由 Skill 在启动 Payload 前明确拒绝，不使用用户环境作为隐式降级路径。

Windows 依赖闭包不使用 CMake `file(GET_RUNTIME_DEPENDENCIES)`：其[文档化的 Windows 搜索顺序](https://cmake.org/cmake/help/latest/command/file.html#get-runtime-dependencies)在依赖文件同目录之后先搜索 `System32` 与 Windows 目录，最后才搜索调用方提供的 `DIRECTORIES`。KAT 必须让 Bundled Python Host 根目录和 native wheel 自有的私有 DLL 目录优先于构建机已安装的系统级 VC Runtime，否则构建可以错误地由 Builder 状态满足、却在干净客户端缺失。[PR #160 的 Round 1 评审修复证据](https://github.com/maokelong/kat-rs/pull/160#round-1-评审修复证据)记录了精确版本、提交、artifact、复现条件与结果；这些是选型证据，不是需要长期维持的架构约束。Windows Payload Builder 按最终 Payload 的目录规则解析依赖，只接受 Payload 已有文件、锁定的 Microsoft 可再分发来源或经文件身份确认的 Windows 系统组件；未解析项与冲突均失败。

Windows 载荷不运行 `VC_redist`、不修改系统状态、不要求管理员权限，也不假设目标机器已经安装系统级 VC Runtime；这些 app-local DLL 与 KAT Skill 其余内容一起原子更新。发布验证必须在未预装额外开发工具或运行库的干净 Windows 环境完成一次完整的 Import → Run → Query 垂直链路，以验证最终原生依赖闭包，而不只验证 `kat.exe` 能启动。

KAT 平台自身在执行时不修改 Skill、Platform Payload 或 PACK 源码，所有平台产生的持久状态都写入 KAT Data Home；这不是文件系统只读强制或针对受信任 PACK 的写入沙箱。KAT CLI 使用 `directories::ProjectDirs::from("", "", "KAT")` 解析 Linux 和 Windows 的平台标准项目数据目录，并将其作为 KAT Data Home。Linux 使用 `$XDG_DATA_HOME/kat` 或 `$HOME/.local/share/kat`，Windows 使用 `%APPDATA%\KAT\data`；不再使用维护者名或 `kat-rs` 作为产品身份，也不迁移或回退读取旧目录。仅“Data Home 只能使用此平台默认目录且没有覆盖来源”的决策由 [ADR-0060](0060-file-and-environment-select-kat-data-home.md) 取代；运行时不修改 Skill、Payload 或 PACK 的边界及本 ADR 的原子发布决策继续有效。

默认 Dataset、External PACK、Run 和日志分别位于 KAT Data Home 的 `datasets/`、`packs/`、`runs/` 和 `logs/`。用户可以为 Data Import 选择其他 Dataset 位置；Run 和日志由 KAT 自行创建和管理，不要求用户组装内部路径。这使 Skill 可以通过整体替换升级，不与可写用户状态相互污染。
