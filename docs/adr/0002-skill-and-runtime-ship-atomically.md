---
status: accepted
---

# Skill 与运行时原子发布

KAT 只把 Skill constraints 作为稳定产品面，并将 Skill 定义、`SKILL.md` 中的平台选择逻辑、KAT CLI、Bundled Python Host 与所支持平台的二进制载荷按同一版本原子发布。这一选择不为底层 CLI 参数承诺独立兼容性，以避免 Skill 与二进制独立升级造成的协议错配，代价是普通终端用户不获得一个单独承诺稳定的 CLI 产品面。

发布采用两阶段构建：`kat/platform/workflow` 先通过成熟 PEP 517 backend 构建一个私有纯 Python Workflow Host wheel，同时包含 PACK 使用的 `import kat` 与 CLI 启动的私有 Runtime module。Linux 与 Windows 的 Platform Payload Builder 在原生平台使用 Cargo 构建 KAT CLI，并以固定版本的 `uv` 获取 `python-build-standalone` 提供的 CPython 3.14 standard-GIL 可重定位目录、把这一个 KAT wheel 与锁文件中的平台第三方预编译 wheels 安装进 Host，生成包含完整标准库、Workflow Runtime、Pack Authoring API、Click、PyArrow、DataFusion 与测试依赖的私有 Python Host。每个 KAT 版本精确锁定 CPython `3.14.z`、PBS release 与 wheel hashes，构建时不浮动选择 latest；不同时携带 CPython 3.13 或 3.14 free-threaded 变体。它不创建 venv，不冻结 Python 应用，不手工复制 site-packages，也不在用户机器解包、下载或解析依赖。一个只理解标准 Skill 路径映射的 Skill Assembly Adapter 再把两个完整载荷、Skill source 和 Bundled PACK 组合为唯一的 `dist/kat`。该 Adapter 是 deployment view 的唯一写入者，但不替代 Cargo、Python 依赖工具、平台打包器或发布系统，也不在第一版增加版本矩阵、签名、哈希和复杂发布一致性机制。

外层发布生命周期由固定版本的 `dist`（原 cargo-dist）管理，包括版本与 tag、原生目标 runner、local/global artifact jobs、校验和、托管和 GitHub Release。各 Platform Payload Builder 作为 local artifact jobs 运行，Skill Assembly Adapter 作为 global artifact job 汇总两个黑盒载荷；KAT 不手改 `dist` 生成的 release workflow，也不在其上重建发布编排器。`dist` 不理解 Skill anatomy，最终路径映射仍只属于薄 Assembly Adapter。

第一阶段的完整载荷矩阵只覆盖 glibc 2.28 及以上的 Linux x86_64，以及 Windows 10 及以上的 x86_64 客户端（包括 Windows 11）。Linux 下限服从 DataFusion/PyArrow 官方 `manylinux_2_28_x86_64` wheels，不为扩大兼容面自行构建 native wheel；`scripts/targets/linux-x86_64/` 的路径不把 glibc 版本编码成第二层平台身份。Windows 下限服从 CPython 与 Rust MSVC target 共同明确支持的客户端范围，不从 `win_amd64` wheel tag 猜测更旧或更细的 OS build 兼容性；Windows 7/8.1 与 Windows Server 第一版均不支持。Windows Platform Payload Builder 根据最终 CPython、KAT CLI 与 native wheels 的实际依赖，从 Microsoft 官方允许再分发的文件集合中确定并随载荷放置 app-local Visual C++ Runtime DLL 闭包，不维护一份脱离产物的固定 DLL 清单。musl、更旧 glibc 和其他不支持的平台由 Skill 在启动 Payload 前明确拒绝，不使用用户环境作为隐式降级路径。

Windows 载荷不运行 `VC_redist`、不修改系统状态、不要求管理员权限，也不假设目标机器已经安装系统级 VC Runtime；这些 app-local DLL 与 KAT Skill 其余内容一起原子更新。发布验证必须在未预装额外开发工具或运行库的干净 Windows 环境完成一次完整的 Import → Run → Query 垂直链路，以验证最终原生依赖闭包，而不只验证 `kat.exe` 能启动。

KAT 平台自身在执行时不修改 Skill、Platform Payload 或 PACK 源码，所有平台产生的持久状态都写入 KAT Data Home；这不是文件系统只读强制或针对受信任 PACK 的写入沙箱。KAT CLI 使用 `directories::ProjectDirs::from("", "", "KAT")` 解析 Linux 和 Windows 的平台标准项目数据目录，并将其作为 KAT Data Home。Linux 使用 `$XDG_DATA_HOME/kat` 或 `$HOME/.local/share/kat`，Windows 使用 `%APPDATA%\KAT\data`；不再使用维护者名或 `kat-rs` 作为产品身份，也不迁移或回退读取旧目录。仅“Data Home 只能使用此平台默认目录且没有覆盖来源”的决策由 [ADR-0060](0060-file-and-environment-select-kat-data-home.md) 取代；运行时不修改 Skill、Payload 或 PACK 的边界及本 ADR 的原子发布决策继续有效。

默认 Dataset、External PACK、Run 和日志分别位于 KAT Data Home 的 `datasets/`、`packs/`、`runs/` 和 `logs/`。用户可以为 Data Import 选择其他 Dataset 位置；Run 和日志由 KAT 自行创建和管理，不要求用户组装内部路径。这使 Skill 可以通过整体替换升级，不与可写用户状态相互污染。
