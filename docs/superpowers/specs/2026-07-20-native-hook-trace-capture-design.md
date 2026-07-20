# Native Hook 计算器 Trace 采集设计

关联 Issue：[#152](https://github.com/maokelong/kat-rs/issues/152)

## 背景与目标

Native Hook 分析需要一条可重复的真机采集路径。本切片使用 Hypium Python 控制系统自带的分布式计算器，在 hiprofiler 采集期间执行 `100 * 100 = 10000`，并保存 trace、SQLite 数据库和可复核日志。

## 不做什么

1. 不实现符号解析或 Excel 导出；这些能力由独立的符号化 PR 交付。
2. 不使用 hmdriver2，也不使用坐标点击兜底。
3. 不创建、构建或安装业务 HAP、测试 HAP；直接操作设备已有的 `ohos.samples.distributedcalc`。
4. 不把穿刺扩展为完整的计算器功能测试。

## Hypium Python 穿刺

采集脚本通过 `hypium.UiDriver.connect(device_sn=...)` 显式连接已选设备，启动 `ohos.samples.distributedcalc/MainAbility`，再按组件 ID 依次点击 `C`、`1`、`0`、`0`、`*`、`1`、`0`、`0`、`=`。最后读取 `expression` 的文本并断言为 `10000`。

这条路径只依赖设备上已有的计算器，不需要额外 HAP。Hypium Python 首次连接可能向设备部署与系统 `uitest` 配套的框架 agent；该 agent 是 Hypium 的 UI 自动化运行组件，不是待测应用或测试 HAP。

安装固定版本的 Python 依赖：

```powershell
py -m venv target/hypium-venv
target/hypium-venv/Scripts/python.exe -m pip install -i https://pypi.org/simple -r tools/requirements-native-hook-capture.txt
```

不要向系统 Python 安装这些依赖。`target/` 已被 Git 忽略，删除 `target/hypium-venv` 即可完整移除该环境，不影响机器上已有的 Python 包。

## Python 采集入口

```text
target/hypium-venv/Scripts/python.exe tools/native_hook_capture.py [--duration 秒] [--target 序列号] [--hdc 路径] [--trace-streamer 路径] [--output-root 目录]
```

必须使用安装了 `tools/requirements-native-hook-capture.txt` 的 Python。`hdc` 和 `trace_streamer_windows.exe` 默认从 `PATH` 查找。省略 `--target` 时必须恰有一个 Connected 设备。hiprofiler 配置内置，目标进程固定为 `ohos.samples.distributedcalc`，时长默认 30 秒。

脚本先完成 Hypium 导入和设备连接，避免首次初始化占用采集窗口；随后启动 profiler 并最多轮询 10 秒，要求进程仍在运行且远端 trace 已存在并大于零，再执行计算器操作。只有 profiler 正常退出且计算结果为 `10000` 时，才拉取 trace 并调用 trace_streamer。

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

远端 trace 使用唯一临时文件名，成功下载后删除。异常时保留已有本地产物；截图失败不得覆盖原始错误。Hypium 导入失败、连接失败、控件缺失或结果不为 `10000` 均为致命错误。trace_streamer 非零退出、数据库缺失或数据库为空同样为致命错误。

## 验收与验证

1. Python 单元测试覆盖设备选择、Hypium 连接参数、点击顺序、结果判定、profiler 就绪和转换错误边界。
2. 在目标真机上验证 Hypium Python 能直接控制预装计算器，不安装额外 HAP。
3. 真机验证 profiler 就绪后执行计算器穿刺，完成 60 秒和 120 秒采集、拉取 trace 并转换 SQLite。
4. 提交前运行：

```text
python -m py_compile tools/native_hook_capture.py tools/test_native_hook_capture.py
python tools/test_native_hook_capture.py
git diff --check
```

## 当前真机验证记录

目标设备 `150100424a544434520325834ac94900` 上已确认预装 `ohos.samples.distributedcalc`，系统 `uitest` 版本为 `7.0.0.1`。Hypium `6.1.0.210` 能识别并连接该设备，但设备端 `uitest start-daemon singleness` 未能拉起 UI RPC，框架返回 `OHOSRpcProcessNotFindError`。因此当前设备尚未完成 UI 穿刺以及 60 秒、120 秒有效采集；不能把先前由缺失测试 HAP 导致的微小 trace 当作验证证据。

## 最小交付切片

1. 提供直接控制预装计算器的 Hypium Python 逻辑，不交付 HAP 工程。
2. 提供 Python Native Hook 采集与 trace_streamer 转换入口。
3. 给出自动化测试和可复核的真机验证记录。
