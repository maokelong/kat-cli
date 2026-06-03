# 微信冷启动分析全过程

Trace: `tests/test.htrace`

Trace id: `bytrace:800cad0a647e51b2`

分析策略: `../strategies/cold-start-critical-path-small-core.md`

## 0. 环境与表确认

当前 Web UI 已加载 `tests/test.htrace`，dataset 为 `dataset:bytrace:800cad0a647e51b2`。

可用核心表：

| 表 | 行数 | 用途 |
| --- | ---: | --- |
| `raw_event` | 1,057,058 | 查冷启 tag、系统 marker |
| `thread_state` | 410,687 | 查 running/sleeping/runnable/uninterruptible |
| `sched_slice` | 230,895 | 统计真实 CPU 运行时间 |
| `callstack` | 93,814 | 还原函数级阶段 |
| `process` | 1,612 | 定位进程 |
| `thread` | 1,612 | 定位线程 |

## 1. 定位目标进程

按 `wechat/tencent` 搜索进程，冷启主链路的目标进程为：

| upid | pid | process_name | start_ts | end_ts |
| ---: | ---: | --- | ---: | ---: |
| 329 | 15040 | `.tencent.wechat` | 245644541000 | 257654690000 |

后续冷启 tag 需要按进程过滤。trace 中还有多个 `.tencent.wechat` 或 `wechatlv:*` 进程，但它们启动时间晚于主链路，不能混入这次冷启主路径。

## 2. 查找鸿蒙冷启 tag

策略要求 tag 链：

`touchEventDispatch -> HandleLaunchApplication -> HandleLaunchAbility -> HandleAbilityTransaction -> OnVsyncEvent now`

本 trace 中没有命中精确的 `touchEventDispatch`。按策略标注为缺失，并用最接近目标启动的 `IconStart com.tencent.wechat` 作为替代起点。

后四个 tag 按进程聚合如下：

| tag | upid | pid | process_name | thread_name | count | first_ts | last_ts |
| --- | ---: | ---: | --- | --- | ---: | ---: | ---: |
| `HandleLaunchApplication` | 329 | 15040 | `.tencent.wechat` | `.tencent.wechat` | 1 | 245673968000 | 245673968000 |
| `HandleLaunchApplication` | 451 | 15364 | `OS_FFRT_5_0` | `OS_FFRT_5_0` | 1 | 245675382000 | 245675382000 |
| `HandleLaunchAbility` | 329 | 15040 | `.tencent.wechat` | `.tencent.wechat` | 1 | 245720189000 | 245720189000 |
| `HandleLaunchAbility` | 142 | 12840 | `xtensionAbility` | `xtensionAbility` | 1 | 246210269000 | 246210269000 |
| `HandleAbilityTransaction` | 329 | 15040 | `.tencent.wechat` | `.tencent.wechat` | 2 | 246203757000 | 246203774000 |
| `OnVsyncEvent now` | 207 | 3009 | `ohos.sceneboard` | `ohos.sceneboard` | 30 | 245654165000 | 257858329000 |
| `OnVsyncEvent now` | 329 | 15040 | `.tencent.wechat` | `.tencent.wechat` | 175 | 246306930000 | 251454658000 |

选择结论：

- `touchEventDispatch`: 缺失，用 `IconStart com.tencent.wechat` 替代，`ts=245615162000`。
- `HandleLaunchApplication`: 取微信主进程 `pid=15040`，`ts=245673968000`。
- `HandleLaunchAbility`: 取微信主进程 `pid=15040`，`ts=245720189000`。
- `HandleAbilityTransaction`: 取微信主进程第一个 transaction，`ts=246203757000`。
- `OnVsyncEvent now`: 取微信主进程第一个 `OnVsyncEvent now`，`ts=246306930000`。同名 sceneboard 事件不作为目标 App tag。

## 3. 阶段耗时

由于 `touchEventDispatch` 缺失，总窗口是替代口径：`IconStart -> target OnVsyncEvent now`。

| 阶段 | 起点 | 终点 | 耗时 ms |
| --- | ---: | ---: | ---: |
| A `IconStart -> HandleLaunchApplication` | 245615162000 | 245673968000 | 58.806 |
| B `HandleLaunchApplication -> HandleLaunchAbility` | 245673968000 | 245720189000 | 46.221 |
| C `HandleLaunchAbility -> HandleAbilityTransaction` | 245720189000 | 246203757000 | 483.568 |
| D `HandleAbilityTransaction -> OnVsyncEvent now` | 246203757000 | 246306930000 | 103.173 |
| Total `IconStart -> OnVsyncEvent now` | 245615162000 | 246306930000 | 691.768 |

第一结论：最大耗时段是 C，约 483.568 ms，占替代总窗口约 69.9%。

## 4. 主线程状态分布

目标主线程：`utid=329 / tid=15040 / process=.tencent.wechat`。

| 阶段 | state | 耗时 ms |
| --- | --- | ---: |
| A | running | 20.801 |
| A | sleeping | 7.625 |
| A | runnable | 1.015 |
| B | running | 43.560 |
| B | sleeping | 2.425 |
| B | runnable | 0.230 |
| B | uninterruptible | 0.006 |
| C | running | 470.287 |
| C | sleeping | 11.674 |
| C | runnable | 5.304 |
| C | uninterruptible | 0.259 |
| D | running | 86.060 |
| D | sleeping | 15.220 |
| D | runnable | 1.729 |
| D | uninterruptible | 0.164 |

汇总：

| state | 耗时 ms |
| --- | ---: |
| running | 620.708 |
| sleeping | 36.944 |
| runnable | 8.278 |
| uninterruptible | 0.429 |

判断：主线程主要是在 running，不是等待态主导，也不是 runnable 调度延迟主导。

## 5. 关键路径 callstack

C 段主耗时在微信主线程，关键嵌套如下：

| span | ts | dur ms | 说明 |
| --- | ---: | ---: | --- |
| `MainThread::HandleLaunchAbility` | 245720189000 | 481.901 | C 段主入口 |
| `AbilityThread::AbilityThreadMain` | 245763334000 | 438.753 | Ability 线程主初始化 |
| `UIAbilityThread::Attach` | 245763388000 | 438.698 | UIAbility attach |
| `UIAbilityImpl::Init` | 245763505000 | 437.870 | UIAbility 初始化 |
| `JsUIAbility::Init` | 245763520000 | 437.852 | JS Ability 初始化 |
| `JsRuntime::LoadModule` | 245764223000 | 436.516 | 加载模块 |
| `JsRuntime::RunScript` | 245764243000 | 436.401 | 执行 JS |
| `EntryAbility.abc ExecuteModuleBufferSecure` | 245764322000 | 436.315 | 执行 EntryAbility.abc |
| `SourceTextModule::Evaluate EntryAbility` | 245764334000 | 436.295 | EntryAbility 模块求值 |
| `aff_biz / libaff_biz.so` | 245764419000 | 110.895 / 110.775 | 业务库加载/初始化 |
| `feat_weapp / libfeat_weapp.so` | 246024113000 | 56.378 / 56.263 | 小程序相关能力 |
| `ohos_base / libohos_base.so` | 245880188000 | 23.299 / 23.218 | 基础库 |

D 段关键嵌套：

| span | ts | dur ms | 说明 |
| --- | ---: | ---: | --- |
| `ScheduleAbilityTransaction` | 246203504000 | 59.335 | transaction 调度 |
| `UIAbilityImpl::HandleAbilityTransaction` | 246203774000 | 59.048 | transaction 处理 |
| `UIAbilityImpl::Start` | 246203795000 | 59.026 | start |
| `JsUIAbility::OnStart` | 246203797000 | 42.660 | JS OnStart |
| `CallObjectMethod:onCreate` | 246204354000 | 26.475 | onCreate |
| `UI Initialize:wechat0` | 246264566000 | 18.130 | UI 初始化 |
| `load page: WechatApp(id:1)` | 246289465000 | 4.055 | 页面加载 |
| `ReceiveVsync WM_15040` | 246306873000 | 25.646 | 目标窗口 Vsync |

关键路径判断：当前证据下的关键路径主要是微信主线程执行 `HandleLaunchAbility -> JsUIAbility::Init -> EntryAbility.abc evaluate`，而不是等待其他线程。

## 6. CPU 簇映射

本 trace 没有直接导出的 `cpu_frequency` 表；`raw_event.clock_set_rate` 中有频率事件，但值更像平台编码。这里采用平台/trace 推断映射：

| CPU | cluster |
| --- | --- |
| 0-3 | small |
| 4-9 | middle |
| 10-11 | big |

该映射是本报告的小核时间计算口径，需要在正式交付中用设备拓扑或频率表二次确认。

## 7. 关键路径 CPU 运行时间

保守关键路径口径：后四个目标 tag 和重型 callstack 都在微信主线程，因此 `path_span` 使用微信主线程 `utid=329` 在 A/B/C/D 阶段的运行片段。A 段中 `IconStart -> 进程创建` 的系统侧工作不计入微信主线程小核时间。

按阶段和 CPU 簇：

| 阶段 | big ms | middle ms | small ms |
| --- | ---: | ---: | ---: |
| A | 0.530 | 19.321 | 0.950 |
| B | 43.091 | 0.469 | 0.000 |
| C | 470.158 | 0.129 | 0.000 |
| D | 81.429 | 4.631 | 0.000 |

总计：

| cluster | running_ms |
| --- | ---: |
| big | 595.208 |
| middle | 24.550 |
| small | 0.950 |

小核占比：

`0.950 / (595.208 + 24.550 + 0.950) = 0.153%`

## 8. 结论

1. 本 trace 缺失精确 `touchEventDispatch`，因此用 `IconStart com.tencent.wechat` 作为替代起点。替代窗口 `IconStart -> 微信主进程首个 OnVsyncEvent now` 耗时约 691.768 ms。
2. 后四个冷启 tag 均按进程区分，主链路选择微信主进程 `pid=15040/upid=329`，排除了 sceneboard 的同名 `OnVsyncEvent now`。
3. 最大耗时阶段是 `HandleLaunchAbility -> HandleAbilityTransaction`，耗时 483.568 ms。
4. 关键路径主要在微信主线程，主线程 C 段 running 470.287 ms，等待和 runnable 延迟都不高。
5. C 段核心耗时是 `EntryAbility.abc` 的 JS 模块加载与求值，特别是 `JsRuntime::LoadModule/RunScript`、`SourceTextModule::Evaluate EntryAbility`，以及 `aff_biz`、`feat_weapp` 等业务库/模块初始化。
6. 关键路径小核运行时间约 0.950 ms，占主线程关键路径运行时间约 0.153%。当前证据不支持“冷启慢主要因为关键路径跑在小核上”。

## 9. 后续建议

1. 优先优化 C 段：减少 `EntryAbility.abc` 首启同步加载/求值量，拆分或延迟 `aff_biz`、`feat_weapp`、`ohos_base` 等模块。
2. D 段关注 `onCreate`、`UI Initialize:wechat0` 和首个 `ReceiveVsync WM_15040` 前的 UI 构建链路。
3. 如果需要严谨输入侧起点，需要采集包含 `touchEventDispatch` 的 trace，或者在当前 trace 中补充输入事件到 IconStart 的确定性关联。
4. 如果要正式量化小核问题，需要用设备拓扑或明确频率表确认 CPU0-3/4-9/10-11 的簇映射。
