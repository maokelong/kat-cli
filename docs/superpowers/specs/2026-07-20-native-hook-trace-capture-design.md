# Native Hook 计算器 Trace 采集设计

关联 Issue：[#152](https://github.com/maokelong/kat-rs/issues/152)

## 背景与目标

Native Hook 分析需要一条可重复的真机采集路径。本切片使用 Python 编排系统自带的 UiTest 命令，控制预装的分布式计算器，在 hiprofiler 采集期间执行 `100 * 100 = 10000`，并保存 trace、SQLite 数据库和可复核日志。

## 不做什么

1. 不实现符号解析或 Excel 导出；这些能力由独立的符号化 PR 交付。
2. 不使用 hmdriver2，不硬编码屏幕坐标。
3. 不创建、构建或安装业务 HAP、测试 HAP；直接操作设备已有的 `ohos.samples.distributedcalc`。
4. 不依赖 Hypium Python Driver；当前 RK3568 镜像无法加载公开版 Hypium 的设备运行库。
5. 不把穿刺扩展为完整的计算器功能测试。

## 原生 UiTest 穿刺

脚本通过 HDC 显式连接已选设备，停止并启动 `ohos.samples.distributedcalc/MainAbility`。随后调用：

```text
uitest dumpLayout -p <临时文件> -b ohos.samples.distributedcalc
```

布局是 JSON 树。脚本按组件 `id` 查找 `C`、`1`、`0`、`*`、`=`，要求每个 ID 只匹配一个可见、启用、可点击的组件，解析其 `bounds` 并计算中心坐标，再调用：

```text
uitest uiInput click <x> <y>
```

点击顺序为 `C`、`1`、`0`、`0`、`*`、`1`、`0`、`0`、`=`。操作后重新获取布局，读取 `expression` 的 `text` 并断言为 `10000`。坐标全部来自本次布局，不在脚本中固化。

## Python 采集入口

```text
python tools/native_hook_capture.py [--duration 秒] [--target 序列号] [--hdc 路径] [--trace-streamer 路径] [--output-root 目录]
```

脚本只使用 Python 标准库，不需要额外 Python 依赖。`hdc` 和 `trace_streamer_windows.exe` 默认从 `PATH` 查找。省略 `--target` 时必须恰有一个 Connected 设备。hiprofiler 配置内置，目标进程固定为 `ohos.samples.distributedcalc`，时长默认 30 秒。

脚本先启动计算器并确认布局完整，再启动 profiler，避免 Native Hook 连接期间的进程启动阻塞影响页面渲染。profiler 启动后最多轮询 10 秒，要求进程仍在运行且远端 trace 已存在并大于零，再执行计算器操作。只有 profiler 正常退出且计算结果为 `10000` 时，才拉取 trace 并调用 trace_streamer。

## 运行产物与错误

每次运行目录为 `target/trace/YYYYMMDD-HHMMSS`，重名时追加 `-01`、`-02`：

```text
native_heap.htrace
trace.db
hiprofiler.log
uitest.log
trace-streamer.log
failure.png        # 仅 UiTest 或 UI 异常时尽力生成
```

远端布局和 trace 使用唯一临时文件名并在结束时清理。异常时保留已有本地产物；截图失败不得覆盖原始错误。布局命令失败、JSON 无效、组件缺失或重复、边界无效、结果不为 `10000` 均为致命错误。trace_streamer 非零退出、数据库缺失或数据库为空同样为致命错误。

## 验收与验证

1. Python 单元测试覆盖设备选择、布局遍历、组件唯一性、边界解析、点击顺序、结果判定、profiler 就绪和转换错误边界。
2. 在 API 26、`uitest 7.0.0.1` 的 RK3568 真机上直接控制预装计算器，不安装额外 HAP。
3. 真机验证 profiler 就绪后执行计算器穿刺，完成 60 秒和 120 秒采集、拉取 trace 并转换 SQLite。
4. 提交前运行：

```text
python -m py_compile tools/native_hook_capture.py tools/test_native_hook_capture.py
python tools/test_native_hook_capture.py
git diff --check
```

## 最小交付切片

1. 提供直接控制预装计算器的原生 UiTest Python 逻辑。
2. 提供 Python Native Hook 采集与 trace_streamer 转换入口。
3. 给出自动化测试和真机验证证据。

## 真机验证记录

目标设备：`7001005458323933328a521c3c503800`，API 26，`uitest 7.0.0.1`。

| 配置时长 | 数据库时间范围 | htrace 大小 | trace.db 大小 | native_hook 行数 | 结果 |
| --- | ---: | ---: | ---: | ---: | --- |
| 60 秒 | 59.82 秒 | 44,523,108 B | 27,422,720 B | 193,241 | `10000` |
| 120 秒 | 119.82 秒 | 44,362,838 B | 27,185,152 B | 189,179 | `10000` |

两次均完成 trace_streamer 转换。文件大小相近，但数据库 `trace_range` 分别覆盖约 60 秒和 120 秒，未发生采集时长截断。
