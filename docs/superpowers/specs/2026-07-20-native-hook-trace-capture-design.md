# Native Hook 应用 Trace 采集设计

关联 Issue：[#152](https://github.com/maokelong/kat-rs/issues/152)

## 背景与目标

Native Hook 分析需要一条可重复的真机采集路径。本切片使用 Python 编排系统自带的 UiTest 命令，启动目标应用并在 hiprofiler 采集期间产生轻量 UI 负载，核心产物是可供后续符号转换使用的 trace。SQLite 数据库仅作为当前串联、验证符号转换流程的胶水产物。

## 不做什么

1. 不实现符号解析或 Excel 导出；这些能力由独立的符号化 PR 交付。
2. 不使用 hmdriver2，不硬编码屏幕坐标。
3. 不创建、构建或安装业务 HAP、测试 HAP；直接操作设备已有的 `ohos.samples.distributedcalc`。
4. 不依赖 Hypium Python Driver；当前 RK3568 镜像无法加载公开版 Hypium 的设备运行库。
5. 不把应用操作扩展为功能正确性测试；除场景显式声明的启动准备外，随机负载是否完成不决定 Trace 成败。
6. 本切片不实现未知应用的操作逻辑，不引入动态插件、外部场景配置文件或脚本加载机制。

## 模块 seam

采集入口只依赖应用场景的两个标识和两个行为：场景名称、目标进程 Bundle，以及“采集前准备”和“采集期间操作”。设备选择、hiprofiler 生命周期、Trace 拉取、SQLite 转换、失败截图和产物目录均留在通用采集流程内。

当前提供两个 adapter：

1. `calculator`：准备阶段负责启动预装计算器并校验 `100 * 100 = 10000`，操作阶段负责动态发现安全按钮并随机点击到 profiler 退出。
2. `note`：准备阶段负责启动 `com.ohos.note/MainAbility`、确认进入 `pages/MyNoteHome`，并在搜索框内部动态计算左、中、右三个安全点；操作阶段使用这组进程内缓存坐标随机点击，不再依赖 Native Hook 期间可能失败的布局导出，也不发送返回键。菜单、新增笔记按钮和笔记条目不进入随机集合，不创建、编辑或删除笔记。

命令行通过 `--scenario` 选择 adapter，默认仍为 `calculator`，保持现有调用兼容。后续支持其他应用时，应新增该应用的 adapter 并注册场景，不复制或修改通用采集流程。

## 原生 UiTest 穿刺

脚本通过 HDC 显式连接已选设备，停止并启动 `ohos.samples.distributedcalc/MainAbility`。随后调用：

```text
uitest dumpLayout -p <临时文件> -b ohos.samples.distributedcalc
```

布局是 JSON 树。脚本按组件 `id` 查找 `C`、`1`、`0`、`*`、`=`，要求每个 ID 只匹配一个可见、启用、可点击的组件，解析其 `bounds` 并计算中心坐标，再调用：

```text
uitest uiInput click <x> <y>
```

首先按 `C`、`1`、`0`、`0`、`*`、`1`、`0`、`0`、`=` 完成启动校验。操作后重新获取布局，读取 `expression` 的 `text` 并断言为 `10000`。

校验通过后，脚本从当前布局中收集所有 `type=Button` 且可见、启用、可点击、具有非空 ID 和有效边界的计算器按钮，持续随机选择按钮并点击，直到 hiprofiler 进程退出。随机种子、候选按钮和点击总数写入 `uitest.log`。坐标全部来自本次布局，不在脚本中固化。

## Python 采集入口

```text
python tools/native_hook_capture.py [--scenario calculator|note] [--duration 秒] [--target 序列号] [--hdc 路径] [--trace-streamer 路径] [--output-root 目录]
```

脚本只使用 Python 标准库，不需要额外 Python 依赖。`hdc` 和 `trace_streamer_windows.exe` 默认从 `PATH` 查找。省略 `--target` 时必须恰有一个 Connected 设备。`--scenario` 默认选择 `calculator`，hiprofiler 根据所选场景的 Bundle 确定目标进程，时长默认 30 秒。

脚本先执行所选场景的必要准备：启动目标应用，并完成该场景显式要求的页面或结果校验；准备失败意味着目标进程不可采集，属于致命错误。随后启动 profiler，最多轮询 10 秒，要求进程仍在运行且远端 trace 已存在并大于零，再执行随机负载。采集期间的随机操作只是制造负载，失败时写入告警但不否定本次 Trace；profiler 正常退出后仍拉取 trace 并调用 trace_streamer。

## 运行产物与错误

每次运行目录为 `target/trace/YYYYMMDD-HHMMSS`，重名时追加 `-01`、`-02`：

```text
native_heap.htrace
trace.db
hiprofiler.log
uitest.log
trace-streamer.log
```

远端布局和 trace 使用唯一临时文件名，不会与后续采集冲突。成功拉取 trace 后删除远端文件；失败运行允许保留远端 trace，代价只是占用设备空间。截图仅在设备支持时尽力生成，不属于验收产物，也不得覆盖原始错误。场景准备阶段的布局、组件或结果校验失败为致命错误；采集阶段的随机操作失败只记录告警。trace_streamer 和 SQLite/Excel 是串联 Trace 与符号转换的验证胶水，不按生产级数据管道完善。

## 验收与验证

1. Python 单元测试覆盖设备选择、布局遍历、组件唯一性、边界解析、启动点击顺序、随机按钮筛选、随机负载结束条件、结果判定和 profiler 就绪边界。
2. 在 API 26、`uitest 7.0.0.1` 的 RK3568 真机上直接控制预装计算器，不安装额外 HAP。
3. 真机先完成计算器启动校验，再验证 profiler 采集期间持续随机点击；完成 30 秒、60 秒和 120 秒采集、拉取 trace 并转换 SQLite。
4. 提交前运行：

```text
python -m py_compile tools/native_hook_capture.py tools/test_native_hook_capture.py
python tools/test_native_hook_capture.py
git diff --check
```

## 最小交付切片

1. 提供计算器和备忘录两个应用场景，验证场景 adapter 能为目标进程制造轻量负载。
2. 提供 Python Native Hook 采集与 trace_streamer 转换入口。
3. 给出自动化测试和真机验证证据。

## 真机验证记录

目标设备：`7001005458323933328a521c3c503800`，API 26，`uitest 7.0.0.1`。

| 场景 | 配置时长 | 数据库时间范围 | htrace 大小 | trace.db 大小 | native_hook 行数 | 结果 | 随机点击 |
| --- | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| `calculator` | 30 秒 | 29.83 秒 | 42,669,350 B | 16,986,112 B | 182,324 | `10000` | 26 |
| `calculator` | 60 秒 | 59.89 秒 | 50,121,912 B | 28,323,840 B | 362,378 | `10000` | 26 |
| `calculator` | 120 秒 | 119.83 秒 | 58,813,446 B | 42,143,744 B | 572,425 | `10000` | 81 |
| `note` | 30 秒 | 28.20 秒 | 45,050,281 B | 29,716,480 B | 143,913 | 首页搜索框 | 14 |

四次均完成 trace_streamer 转换。`calculator` 在采集窗口内动态识别 18 个按钮；`note` 在采集前缓存三个搜索框安全点，采集期间不修改笔记数据。备忘录数据库的 143,913 条 `native_hook` 记录全部关联 `process.name=com.ohos.note`。随机种子、候选操作和点击总数均写入 `uitest.log`。
