# Native Hook 符号化设计

关联 Issue：[#152](https://github.com/maokelong/kat-rs/issues/152)

## 背景与目标

Native Hook trace 中的调用帧可能只包含 `/system/lib/ndk/libffrt.so+0x74825` 一类模块相对虚拟地址。本切片使用本地未裁剪 ELF 把地址转换为稳定、可读的函数符号，并提供 trace_streamer SQLite 到 Excel 的分析入口。

## 不做什么

1. 不负责真机 trace 采集或计算器自动化；这些能力由独立的采集 PR 交付。
2. 不提供 C/C++ FFI，也不保留 PoC 的 JSONL/单 ELF 命令面。
3. 不实现全局单例或跨进程持久化缓存；需要复用时由调用方显式持有 `SymbolResolver`。
4. 不跟随目录符号链接或 junction，不承担历史运行目录的迁移和恢复。

## 符号化契约

公开 Rust 接口：

```rust
pub struct SymbolizationResult {
    pub symbols: Vec<String>,
    pub missing_modules: Vec<MissingModule>,
}

pub struct SymbolResolver { /* private fields */ }

impl SymbolResolver {
    pub fn new(symbol_dir: impl Into<PathBuf>) -> Self;

    pub fn get_symbols(
        &mut self,
        addr_list: &[String],
        module_name_map: &HashMap<String, String>,
        include_source_location: bool,
    ) -> anyhow::Result<SymbolizationResult>;
}

pub fn get_symbols(
    addr_list: &[String],
    symbol_dir: &Path,
    module_name_map: &HashMap<String, String>,
    include_source_location: bool,
) -> anyhow::Result<SymbolizationResult>;
```

`SymbolResolver` 首次遇到合法查询时递归建立一次 SO basename 到完整路径列表的索引，后续调用复用该索引、模块命中/缺失结果以及 `Symbolizer`。实例生命周期内符号目录视为稳定；目录更新后调用方应创建新实例。兼容函数 `get_symbols` 每次创建临时实例，适合单次调用。

空输入直接返回两个空列表；全部输入均不合法时原样返回，二者都不扫描或校验 `symbol_dir`。存在合法查询时要求根目录存在、是目录且可打开；不可访问的子项告警后跳过。`symbols` 与输入等长且顺序一致；`missing_modules` 按规范化模块路径排序，记录本地没有匹配 SO 的路径和出现次数。

合法查询去除首尾空白后必须满足：非空模块路径以 `.so` 或 `.so.N...` 结尾，随后是 `+0xHEX`。解析失败、模块缺失或地址无符号时输出原始字符串。

`module_name_map` 的方向为“trace 中的 SO 名称或路径 → 符号目录中的 SO 名称或路径”。完整设备路径键优先于 basename 键；映射后的目标继续按完整路径后缀、basename 的顺序匹配符号文件。多个候选按规范化完整路径字典序选择第一个并告警。

地址固定按 `blazesym::symbolize::Input::VirtOffset` 解析。成功结果为 `function_name+0xoffset`；开启源码位置时为：

```text
outer+0x27 (/full/path/outer.cpp:50) => middle_inline (/full/path/middle.h:80) => inner_inline (/full/path/inner.h:21)
```

内联链按外到内展开，只有主符号带偏移，不输出列号。缺少 DWARF 时退化为基本符号；demangle 失败时保留原始符号名。同一 ELF 和地址在单次调用内去重。

## `get_symbols` 处理流程

```mermaid
flowchart TD
    A["输入地址列表"]
    B["解析地址并应用 SO 名称映射"]
    C{"符号目录索引是否已建立？"}
    D["扫描符号目录并建立 SO 路径索引"]
    E["查找对应符号文件"]
    F["按 SO 分组，相同地址去重"]
    G["每个 SO 批量转换符号"]
    H["按原始顺序回填结果"]
    I["返回符号结果和缺失 SO 列表"]

    A --> B --> C
    C -- "否" --> D --> E
    C -- "是" --> E
    E --> F --> G --> H --> I
```

目录索引在 `SymbolResolver::get_symbols` 首次遇到合法地址时建立，而不是在创建 `SymbolResolver` 时建立。相同 SO 的地址会集中批量转换，相同地址在单次调用内只解析一次。

## SQLite 与 Excel

Rust CLI：

```text
kat-native-hook-symbolize <trace.db> --symbol-dir <目录> --output <symbols.xlsx> [--module-map <FROM=TO>]... [--include-source-location]
```

使用 `rusqlite` 读取、`rust_xlsxwriter` 写入，输出文件存在时覆盖。查询保留全部 `native_hook_frame`，按 `callchain_id`、`depth`、`id` 升序。

`original_symbol` 优先取 `symbol_id -> data_dict.data`；缺失时用 `file_id -> data_dict.data` 与 `vaddr` 拼成 `模块+0x地址`；两者均不可用时为空。`symbols` 工作表列为：

1. `callchain_id`
2. `depth`
3. `original_symbol`
4. `resolved_symbol`

首行冻结并启用筛选。超过 1,048,575 条数据行时依次拆分为 `symbols_1`、`symbols_2`。另建 `missing_modules` 工作表汇总本地未匹配 SO；SO 已找到但地址无符号时不进入该表。合法空帧表生成仅含表头的工作簿并告警。

## 验收与验证

1. 单元测试覆盖严格语法、原样回退、顺序和重复项、确定性歧义选择、根目录与子项错误、延迟索引和跨调用复用。
2. 单元测试覆盖数据库取值优先级、稳定排序、空表以及 Excel 分页命名。
3. 使用实际 trace SQLite 和 `D:\zxlDown\images\laster` 生成 Excel 并抽查符号结果。
4. 提交前运行：

```text
cargo fmt --all -- --check
cargo test -p kat-rs-native-hook-symbolize-poc
cargo clippy -p kat-rs-native-hook-symbolize-poc --all-targets -- -D warnings
git diff --check
```

## 最小交付切片

1. 收敛 Rust 符号化接口及测试。
2. 增加 SQLite 到 Excel CLI 及测试。
3. 给出实际符号目录和 trace 数据的验证证据。
