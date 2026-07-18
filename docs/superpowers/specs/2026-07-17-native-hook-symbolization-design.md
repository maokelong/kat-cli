# Native Hook 符号化与计算器采集设计

关联 Issue：[#152](https://github.com/maokelong/kat-rs/issues/152)

## 背景

Native Hook trace 中的调用帧可能只包含 `/system/lib/ndk/libffrt.so+0x74825` 一类模块相对虚拟地址。当前 PoC 能直接调用 `blazesym`，但接口、地址语义、模块选择和输出形式均未形成可交付契约，也没有从 trace SQLite 到 Excel 的分析入口或稳定的真机采集脚本。

## 要解决的问题

1. 使用本地未裁剪 ELF，把 Native Hook 模块相对虚拟地址转换为稳定、可读的函数符号。
2. 从 trace_streamer 生成的 SQLite 读取全部 `native_hook_frame`，按调用链顺序输出 Excel。
3. 使用 Hypium `ohosTest` 操作分布式计算器，在 Native Hook 采集期间产生轻量计算负载。
4. 为每次采集保存可复核的 trace、数据库和工具日志；UI 异常时保存现场截图。

## 不做什么

1. 不提供 C/C++ FFI，也不保留 PoC 的 JSONL/单 ELF 命令面。
2. 不实现跨调用或跨进程单例缓存；第一版缓存只在一次 `get_symbols` 调用内有效。
3. 只验证固定穿刺 `100 * 100 = 10000`，不扩展为计算器功能正确性测试或复杂计算场景。
4. 不提供 Python 到 Rust 的一键总编排；采集与分析是两个入口。
5. 不跟随目录符号链接或 junction，不承担历史运行目录的迁移和恢复。

## 符号化契约

公开 Rust 接口：

```rust
pub struct SymbolizationResult {
    pub symbols: Vec<String>,
    pub missing_modules: Vec<MissingModule>,
}

pub fn get_symbols(
    addr_list: &[String],
    symbol_dir: &Path,
    module_name_map: &HashMap<String, String>,
    include_source_location: bool,
) -> anyhow::Result<SymbolizationResult>
```

空输入直接返回两个空列表，不扫描或校验 `symbol_dir`。非空输入要求根目录存在、是目录且可打开；不可访问的子项告警后跳过。`symbols` 与输入等长且顺序一致；`missing_modules` 按规范化模块路径排序，记录本地没有匹配 SO 的路径和出现次数。

`module_name_map` 的方向为“trace 中的 SO 名称或路径 → 符号目录中的 SO 名称或路径”。键和值都必须是合法 SO 名称；查找时完整设备路径键优先于 basename 键。映射后的目标继续按完整路径后缀、basename 的顺序匹配符号文件。未命中映射时保持原有匹配行为；映射目标没有符号文件时，`missing_modules` 仍记录 trace 中的原始模块路径。

合法查询去除首尾空白后必须满足：非空模块路径以 `.so` 或 `.so.N...` 结尾，随后是 `+0xHEX`。解析失败、模块缺失或地址无符号时，输出原始未裁剪字符串。输出长度和顺序始终与输入相同。

地址固定按 `blazesym::symbolize::Input::VirtOffset` 解析，不暴露地址类型参数。成功结果为 `function_name+0xoffset`；开启源码位置时为：

```text
outer+0x27 (/full/path/outer.cpp:50) => middle_inline (/full/path/middle.h:80) => inner_inline (/full/path/inner.h:21)
```

内联链按外到内展开，只有主符号带偏移，不输出列号。缺少 DWARF 时退化为基本符号；demangle 失败时保留原始符号名。

模块路径统一分隔符并应用名称映射后，优先做设备完整路径的后缀匹配，再按 basename 匹配。多个候选按规范化完整路径字典序选择第一个，并向标准错误输出告警。查询按选中 ELF 分组，同一 ELF 和地址在单次调用内去重。

## SQLite 与 Excel

Rust CLI：

```text
kat-native-hook-symbolize <trace.db> --symbol-dir <目录> --output <symbols.xlsx> [--module-map <FROM=TO>]... [--include-source-location]
```

使用 `rusqlite` 读取、`rust_xlsxwriter` 写入，输出文件存在时覆盖。查询保留全部帧并按 `callchain_id`、`depth`、`id` 升序。

`original_symbol` 优先取 `symbol_id -> data_dict.data`；缺失时用 `file_id -> data_dict.data` 与 `vaddr` 拼成 `模块+0x地址`；两者均不可用时为空。工作表列为：

1. `callchain_id`
2. `depth`
3. `original_symbol`
4. `resolved_symbol`

首行冻结并启用筛选。未超过 Excel 上限时工作表名为 `symbols`；超过 1,048,575 条数据行时依次拆分为 `symbols_1`、`symbols_2`。另建 `missing_modules` 工作表，按 `module_path`、`occurrence_count` 汇总本地符号目录中没有匹配文件的 SO；SO 已找到但地址无符号时不进入该表。没有缺失 SO 时该表只保留表头。合法空帧表生成仅含表头的工作簿并告警。数据库结构错误、读取失败或工作簿写入失败为致命错误。

## 设备采集

`tools/hypium_calculator_probe` 提供独立的 Hypium `ohosTest` 工程，使用 `@ohos/hypium` 和 `@kit.TestKit` 操作计算器。主 HAP 与测试 HAP 由使用者预先构建、使用同一调试证书和包含目标设备 UDID 的调试 Profile 签名，并按主包、测试包的顺序安装；`tools/native_hook_capture.py` 不自动安装 SDK、依赖或 HAP。

Hypium 用例停止并启动 `ohos.samples.distributedcalc/MainAbility`，等待计算器页面后，按组件 ID 依次点击 `C`、`1`、`0`、`0`、`*`、`1`、`0`、`0`、`=`，不使用坐标兜底。按下等号后，计算器会把最终值写入 `expression` 并清空预览用的 `result`；用例读取 `expression` 并断言其值为 `10000`，以同时确认应用存活、页面可操作和本轮负载完成。

`hdc` 和 `trace_streamer_windows.exe` 默认从 `PATH` 查找，可用 `--hdc`、`--trace-streamer` 覆盖。`--target` 可显式选择设备；省略时必须恰有一个 Connected 设备。采集脚本在 profiler 就绪后通过目标 `hdc` 执行：

```text
aa test -b com.katrs.hypium.calculator -m entry_test -s unittest OpenHarmonyTestRunner -s class CalculatorHypiumTest -s timeout 30000
```

脚本必须同时检查命令执行状态、`OHOS_REPORT_CODE: 0` 和汇总中的 `Failure: 0, Error: 0, Pass: 1`。不能只依赖 `aa test` 的进程退出码，因为用例断言失败时命令仍可能以进程码 0 结束。Hypium 输出写入本次运行目录；测试失败视为 UI 或应用异常，不继续把本次运行报告为成功。

hiprofiler 配置内置，目标进程固定，时长由 `--duration` 配置且默认 30 秒。启动后最多轮询 10 秒，要求进程仍运行且远端 trace 已存在并大于零，随后才操作应用。只有退出码为 0 才拉取 trace；异常退出不继续解析。

## 运行产物与错误

每次运行目录为 `target/trace/YYYYMMDD-HHMMSS`，重名时追加 `-01`、`-02`：

```text
native_heap.htrace
trace.db
hiprofiler.log
hypium.log
trace-streamer.log
symbols.xlsx       # Rust 分析命令生成
failure.png        # 仅 UI 或应用异常时尽力生成
```

远端 trace 使用唯一临时文件名，成功下载后删除；失败时保留本地已有产物。截图失败不得覆盖原始错误。trace_streamer 退出码非零，或未生成可打开的数据库时失败。

## 验收与验证

1. 单元测试覆盖严格语法、原样回退、顺序和重复项、确定性歧义选择、根目录与子项错误边界。
2. 单元测试覆盖数据库取值优先级、稳定排序、空表以及 Excel 分页命名。
3. 使用 `target/trace/test.db` 和 `D:\zxlDown\images\laster` 生成实际 Excel，并抽查 `libffrt.so` 地址。
4. 构建并安装 Hypium 主 HAP 与测试 HAP，真机运行 `CalculatorHypiumTest`，要求报告为 `Tests run: 1, Failure: 0, Error: 0, Pass: 1`。
5. 真机人工穿刺验证 Hypium 用例在 profiler 就绪后执行、30 秒采集、trace 拉取和 trace_streamer 转换。
6. 提交前运行：

```text
cargo fmt --all -- --check
cargo test -p kat-rs-native-hook-symbolize-poc
cargo clippy -p kat-rs-native-hook-symbolize-poc --all-targets -- -D warnings
git diff --check
```

## 最小交付切片

1. 收敛 Rust 符号化接口及测试。
2. 增加 SQLite 到 Excel CLI 及测试。
3. 增加 Hypium 计算器 `ohosTest`、Python 真机采集编排和使用说明。
4. 使用现有 trace、符号目录和真机分别给出验证证据。
