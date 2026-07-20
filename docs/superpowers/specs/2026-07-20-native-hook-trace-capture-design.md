# Native Hook 计算器 Trace 采集设计

关联 Issue：[#152](https://github.com/maokelong/kat-rs/issues/152)

## 背景与目标

Native Hook 分析需要一条可重复的真机采集路径。本切片使用 Hypium 操作系统自带的分布式计算器，在 hiprofiler 采集期间执行 `100 * 100 = 10000`，并保存 trace、SQLite 数据库和可复核日志。

## 不做什么

1. 不实现符号解析或 Excel 导出；这些能力由独立的符号化 PR 交付。
2. 不使用 hmdriver2，也不使用坐标点击兜底。
3. 不自动安装 SDK、依赖或 HAP；测试宿主和 `ohosTest` HAP 必须预先构建、签名并安装。
4. 不把穿刺扩展为完整的计算器功能测试。

## Hypium 穿刺

`tools/hypium_calculator_probe` 是独立的 Hypium `ohosTest` 工程。用例停止并启动 `ohos.samples.distributedcalc/MainAbility`，等待计算器页面后，按组件 ID 依次点击 `C`、`1`、`0`、`0`、`*`、`1`、`0`、`0`、`=`。

按下等号后，计算器把最终值写入 `expression` 并清空预览用的 `result`。用例读取 `expression` 并断言其值为 `10000`，以同时确认应用存活、页面可操作和本轮负载完成。

测试命令为：

```text
aa test -b com.katrs.hypium.calculator -m entry_test -s unittest OpenHarmonyTestRunner -s class CalculatorHypiumTest -s timeout 30000
```

在 `tools/hypium_calculator_probe` 安装依赖并构建：

```powershell
& 'D:\soft\DevEcoStudio\tools\ohpm\bin\ohpm.bat' install
& 'D:\soft\DevEcoStudio\tools\hvigor\bin\hvigorw.bat' --no-daemon --no-parallel --mode module -p product=default -p module=entry assembleHap
& 'D:\soft\DevEcoStudio\tools\hvigor\bin\hvigorw.bat' --no-daemon --no-parallel --mode module -p product=default -p module=entry@ohosTest assembleHap
```

主 HAP 和测试 HAP 使用同一调试证书和包含目标设备 UDID 的调试 Profile 签名，按主包、测试包的顺序安装。

脚本同时检查命令执行状态、`OHOS_REPORT_CODE: 0` 和汇总中的 `Failure: 0, Error: 0, Pass: 1`。用例失败时不能仅凭 `aa test` 的进程退出码判定成功。

## Python 采集入口

```text
python tools/native_hook_capture.py [--duration 秒] [--target 序列号] [--hdc 路径] [--trace-streamer 路径] [--output-root 目录]
```

`hdc` 和 `trace_streamer_windows.exe` 默认从 `PATH` 查找。省略 `--target` 时必须恰有一个 Connected 设备。hiprofiler 配置内置，目标进程固定，时长默认 30 秒。

脚本启动 profiler 后最多轮询 10 秒，要求进程仍在运行且远端 trace 已存在并大于零，随后执行 Hypium 用例。只有 profiler 退出码为 0、Hypium 用例通过时才拉取 trace 并调用 trace_streamer。

## 运行产物与错误

每次运行目录为 `target/trace/YYYYMMDD-HHMMSS`，重名时追加 `-01`、`-02`：

```text
native_heap.htrace
trace.db
hiprofiler.log
hypium.log
trace-streamer.log
failure.png        # 仅 Hypium 或 UI 异常时尽力生成
```

远端 trace 使用唯一临时文件名，成功下载后删除。异常时保留已有本地产物；截图失败不得覆盖原始错误。trace_streamer 非零退出、数据库缺失或数据库为空均为致命错误。

## 验收与验证

1. Python 单元测试覆盖设备选择、参数、Hypium 报告判定、运行目录、profiler 配置和错误边界。
2. Hypium 用例在 API 26 真机报告 `Tests run: 1, Failure: 0, Error: 0, Pass: 1`。
3. 真机验证 profiler 就绪后执行计算器穿刺、完成指定时长采集、拉取 trace 并转换 SQLite。
4. 提交前运行：

```text
python -m py_compile tools/native_hook_capture.py tools/test_native_hook_capture.py
python tools/test_native_hook_capture.py
git diff --check
```

## 最小交付切片

1. 提供 Hypium 计算器穿刺工程。
2. 提供 Python Native Hook 采集与 trace_streamer 转换入口。
3. 给出自动化测试和真机验证证据。
