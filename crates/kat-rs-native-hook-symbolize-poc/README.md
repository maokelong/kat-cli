# Native Hook 符号化

该 crate 将 trace_streamer SQLite 中的 Native Hook 调用帧转换为 Excel。地址固定按 ELF 模块相对虚拟地址处理，无法解析的输入保持原样。

生成的 CLI 二进制名为 `kat-native-hook-symbolize`。

```powershell
cargo run -p kat-rs-native-hook-symbolize-poc --release -- `
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

`symbols` 工作表包含 `callchain_id`、`depth`、`original_symbol` 和 `resolved_symbol`。`missing_modules` 工作表汇总本地符号目录中没有匹配文件的 SO，包含 `module_path` 和 `occurrence_count`；SO 存在但单个地址无符号时不会进入该表。目标文件已存在时覆盖；可恢复的符号目录告警写入标准错误。

## 设备采集

预先构建、签名并安装 `tools/hypium_calculator_probe` 的主 HAP 和测试 HAP，同时确保 `hdc` 和 `trace_streamer_windows.exe` 可执行：

```powershell
py tools/native_hook_capture.py --duration 30
```

多设备场景用 `--target <serial>` 指定设备。外部工具不在 `PATH` 时可传 `--hdc` 和 `--trace-streamer`。脚本在 profiler 就绪后运行 `CalculatorHypiumTest`，由 Hypium 启动 `ohos.samples.distributedcalc`、执行 `100 * 100 =` 并断言最终值为 `10000`。每次运行保存 `hypium.log`；只有 Hypium 或 UI 异常时才尝试保存 `failure.png`。
