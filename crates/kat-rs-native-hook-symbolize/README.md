# Native Hook 符号化

该 crate 提供 Native Hook 模块相对地址的批量符号化 Rust 接口。附带的 CLI 可读取 trace_streamer SQLite，并把符号化结果导出为 Excel；无法解析的输入保持原样。

生成的 CLI 二进制名为 `kat-native-hook-symbolize`。

```powershell
cargo run -p kat-rs-native-hook-symbolize --release -- `
  target/trace/test.db `
  --symbol-dir D:/zxlDown/images/laster `
  --output target/trace/symbols.xlsx
```

需要完整 DWARF 文件、行号和内联链时追加：

```text
--include-source-location
```

trace 中的 SO 名称与符号文件名称不一致时，可重复传入名称映射：

```text
--module-map libtrace.so=libsymbols.so
--module-map /system/lib/libfoo.so=/symbols/libbar.so
```

映射方向为“trace 中的名称或路径 → 符号目录中的名称或路径”。完整路径映射优先于 basename 映射。

## 项目内调用 `get_symbols`

单次调用可继续使用兼容函数 `get_symbols`。连续处理多批地址时，应在进程内复用同一个 `SymbolResolver`，避免每批重新扫描符号目录：

```rust
use std::collections::HashMap;
use kat_rs_native_hook_symbolize::SymbolResolver;

let mut resolver = SymbolResolver::new(r"D:\zxlDown\images\laster");
let module_name_map = HashMap::new();

let first = resolver.get_symbols(&first_batch, &module_name_map, false)?;
let second = resolver.get_symbols(&second_batch, &module_name_map, false)?;
```

目录索引在首次合法查询时延迟建立，模块命中和缺失也会缓存。`SymbolResolver` 存活期间应把符号目录视为只读；目录内容发生变化后创建新实例。全部输入均不合法时不会扫描目录。

`symbols` 工作表包含 `callchain_id`、`depth`、`input_value` 和 `resolved_or_original`。`callchain_id` 以十进制文本写入，避免 64 位整数被 Excel 数值格式截断；`input_value` 是数据库中的已有符号、合成的 `模块+0x地址` 或空字符串；`resolved_or_original` 在符号化成功时保存函数符号，失败时保留 `input_value`。`missing_modules` 工作表汇总本地符号目录中没有匹配文件的 SO，包含 `module_path` 和 `occurrence_count`；其中 `module_path` 是规范化后的输入模块路径。SO 存在但单个地址无符号时不会进入该表。目标文件已存在时覆盖；可恢复的符号目录告警写入标准错误。
