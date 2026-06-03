# 微信冷启动 Atomic 编排策略执行报告

执行策略: `../strategies/cold-start-atomic-orchestration-strategy.md`

Trace: `tests/test.htrace`

Dataset: `dataset:bytrace:800cad0a647e51b2`

Trace id: `bytrace:800cad0a647e51b2`

时间范围: `244818549000 -> 257898194000`

## 0. S0 范围确认

当前 kat-rs Web UI/API 已加载 `tests/test.htrace`。核心表可用:

| 表 | 行数 |
| --- | ---: |
| `raw_event` | 1,057,058 |
| `thread_state` | 410,687 |
| `sched_slice` | 230,895 |
| `callstack` | 93,814 |
| `args` | 95,643 |
| `data_dict` | 23,633 |
| `process` | 1,612 |
| `thread` | 1,612 |

## 1. S1 目标进程定位

执行能力: `harmony_process_candidates`

按 `wechat/tencent` 搜索后，冷启动主链路目标进程选择:

| upid | pid | process_name | main_utid | main_tid | start_ts | end_ts | confidence |
| ---: | ---: | --- | ---: | ---: | ---: | ---: | --- |
| 329 | 15040 | `.tencent.wechat` | 329 | 15040 | 245644541000 | 257654690000 | high |

说明:

- Trace 中存在多个后续启动的 `.tencent.wechat` 进程，但 `upid=329/pid=15040` 最早覆盖本次冷启动 tag 链路。
- 后续冷启动 tag 除起点外均按 `upid=329` 筛选。

## 2. S2 冷启动 tag 链路还原

执行能力:

- `harmony_cold_start_tag_by_process`
- `harmony_cold_start_anchor_select`

策略要求 tag:

`touchEventDispatch -> HandleLaunchApplication -> HandleLaunchAbility -> HandleAbilityTransaction -> OnVsyncEvent now`

本 trace 未命中精确 `touchEventDispatch`。使用 fallback 起点:

| anchor | ts | upid | pid | process_name | thread_name | confidence |
| --- | ---: | ---: | ---: | --- | --- | --- |
| `IconStart com.tencent.wechat` | 245615162000 | 132 | 15187 | `OS_FFRT_5_47` | `OS_FFRT_5_47` | medium |

目标进程 tag 命中:

| tag | upid | pid | process_name | thread_name | count | first_ts | last_ts |
| --- | ---: | ---: | --- | --- | ---: | ---: | ---: |
| `HandleLaunchApplication` | 329 | 15040 | `.tencent.wechat` | `.tencent.wechat` | 1 | 245673968000 | 245673968000 |
| `HandleLaunchAbility` | 329 | 15040 | `.tencent.wechat` | `.tencent.wechat` | 1 | 245720189000 | 245720189000 |
| `HandleAbilityTransaction` | 329 | 15040 | `.tencent.wechat` | `.tencent.wechat` | 2 | 246203757000 | 246203774000 |
| `OnVsyncEvent now` | 329 | 15040 | `.tencent.wechat` | `.tencent.wechat` | 175 | 246306930000 | 251454658000 |

被排除的同名非目标进程 tag:

| tag | upid | pid | process_name | count | first_ts |
| --- | ---: | ---: | --- | ---: | ---: |
| `OnVsyncEvent now` | 207 | 3009 | `ohos.sceneboard` | 30 | 245654165000 |
| `HandleLaunchApplication` | 451 | 15364 | `OS_FFRT_5_0` | 1 | 245675382000 |
| `HandleLaunchAbility` | 142 | 12840 | `xtensionAbility` | 1 | 246210269000 |

结论:

- 本次总窗口是 fallback window，不是严格 `touchEventDispatch -> 首帧`。
- 后四个冷启动 tag 均在目标进程中完整命中。
- sceneboard 的 `OnVsyncEvent now` 不作为目标 App 首帧终点。

## 3. S3 阶段耗时与 Topdown 判断

执行能力: `harmony_cold_start_phase_breakdown`

| 阶段 | 起点 | 终点 | 耗时 ms |
| --- | ---: | ---: | ---: |
| A `IconStart -> HandleLaunchApplication` | 245615162000 | 245673968000 | 58.806 |
| B `HandleLaunchApplication -> HandleLaunchAbility` | 245673968000 | 245720189000 | 46.221 |
| C `HandleLaunchAbility -> HandleAbilityTransaction` | 245720189000 | 246203757000 | 483.568 |
| D `HandleAbilityTransaction -> OnVsyncEvent now` | 246203757000 | 246306930000 | 103.173 |
| Total `IconStart -> OnVsyncEvent now` | 245615162000 | 246306930000 | 691.768 |

Topdown 判断:

- 最大阶段是 C，耗时 483.568 ms，占 fallback 总窗口约 69.9%。
- 后续优先对 C 段执行任意区间关键路径下钻。

## 4. S4 任意区间关键路径下钻

执行能力: `harmony_process_critical_path_in_range`

分析窗口: C 段 `245720189000 -> 246203757000`

执行中发现一个适配点:

- 当前 trace 的 `callstack.callid` 对应系统 `tid=15040`，不是 `utid=329`。
- 因此本次执行按 `callstack.callid = thread.utid OR callstack.callid = thread.tid` 兼容匹配。
- 已同步修正 `../capabilities/harmony-cold-start-atomics.md` 中的能力说明。

C 段关键路径候选:

| rank | source | kind | thread | span/state | 耗时 ms |
| ---: | --- | --- | --- | --- | ---: |
| 1 | `callstack` | `running_span` | `.tencent.wechat` | `AppSchedulerHost::OnRemoteRequest` clipped to C | 483.568 |
| 2 | `callstack` | `running_span` | `.tencent.wechat` | `MainThread::HandleLaunchAbility` | 481.901 |
| 3 | `callstack` | `running_span` | `.tencent.wechat` | `AbilityThread::AbilityThreadMain` | 438.753 |
| 4 | `callstack` | `running_span` | `.tencent.wechat` | `UIAbilityThread::Attach` | 438.698 |
| 5 | `callstack` | `running_span` | `.tencent.wechat` | `UIAbilityImpl::Init` | 437.870 |
| 6 | `callstack` | `running_span` | `.tencent.wechat` | `JsUIAbility::Init` | 437.852 |
| 7 | `callstack` | `running_span` | `.tencent.wechat` | `JsRuntime::LoadModule` | 436.516 |
| 8 | `callstack` | `running_span` | `.tencent.wechat` | `JsRuntime::LoadJsModule` | 436.410 |
| 9 | `callstack` | `running_span` | `.tencent.wechat` | `JsRuntime::RunScript` | 436.401 |
| 10 | `callstack` | `running_span` | `.tencent.wechat` | `EntryAbility.abc ExecuteModuleBufferSecure` | 436.315 |
| 11 | `callstack` | `running_span` | `.tencent.wechat` | `SourceTextModule::Evaluate EntryAbility` | 436.295 |
| 12 | `callstack` | `running_span` | `.tencent.wechat` | `aff_biz` | 110.895 |
| 14 | `callstack` | `running_span` | `.tencent.wechat` | `feat_weapp` | 56.378 |

判断:

- C 段关键路径在微信主线程。
- 主要是执行型 running span，不是 runnable 调度等待主导，也不是 IO wait 主导。
- 核心耗时链路是 `HandleLaunchAbility -> JsUIAbility::Init -> JsRuntime::LoadModule/RunScript -> EntryAbility.abc Evaluate`。

## 5. S5 状态分布与热点归因

执行能力:

- `harmony_main_thread_states_by_phase`
- `harmony_callstack_hotspots_by_phase`

主线程状态分布:

| 阶段 | running ms | sleeping ms | runnable ms | uninterruptible ms |
| --- | ---: | ---: | ---: | ---: |
| A | 20.801 | 7.625 | 1.015 | 0.000 |
| B | 43.560 | 2.425 | 0.230 | 0.006 |
| C | 470.287 | 11.674 | 5.304 | 0.259 |
| D | 86.060 | 15.220 | 1.729 | 0.164 |

C 段状态判断:

- `running=470.287 ms`，接近 C 段总耗时。
- `runnable=5.304 ms` 很低，不支持“调度排队是主因”。
- `uninterruptible=0.259 ms`，不支持“内核不可中断阻塞是主因”。
- 未看到明确 `io_wait=true` 主导证据。

D 段补充热点:

| span | 耗时 ms |
| --- | ---: |
| `ScheduleAbilityTransaction` | 59.335 |
| `UIAbilityImpl::HandleAbilityTransaction` | 59.048 |
| `UIAbilityImpl::Start` | 59.026 |
| `JsUIAbility::OnStart` | 42.660 |
| `CallObjectMethod:onCreate` | 26.475 |
| `ReceiveVsync WM_15040` | 25.646 |
| `UI Initialize:wechat0` | 18.130 |
| `load page: WechatApp(id:1)` | 4.055 |

D 段有 UI/onStart/onCreate 开销，但不是本次最大阶段。

## 6. S6 小核归因

执行能力:

- `harmony_cpu_cluster_mapping`
- `harmony_critical_path_cpu_cluster_time`

CPU cluster 映射:

| CPU | cluster |
| --- | --- |
| 0-3 | small |
| 4-9 | middle |
| 10-11 | big |

说明: 当前 trace 未直接使用设备拓扑表确认，该映射置信度为 medium。

关键路径线程: 微信主线程 `utid=329/tid=15040`

按阶段 CPU cluster running 时间:

| 阶段 | big ms | middle ms | small ms |
| --- | ---: | ---: | ---: |
| A | 0.530 | 19.321 | 0.950 |
| B | 43.091 | 0.469 | 0.000 |
| C | 470.158 | 0.129 | 0.000 |
| D | 81.429 | 4.631 | 0.000 |

总计:

| cluster | running ms |
| --- | ---: |
| big | 595.208 |
| middle | 24.550 |
| small | 0.950 |

小核占比:

`0.950 / (595.208 + 24.550 + 0.950) = 0.153%`

判断:

- small ratio 远小于 5%。
- 当前 trace 不支持“小核运行是冷启动慢的主因”。
- C 段最大耗时几乎全部运行在 big 核 CPU10。

## 7. 最终结论

1. 本次冷启动分析使用 fallback 起点 `IconStart com.tencent.wechat`，因为 trace 缺少精确 `touchEventDispatch`。fallback 总窗口 `IconStart -> 目标进程 OnVsyncEvent now` 为 691.768 ms。
2. 后四个目标 tag 均在微信主进程 `upid=329/pid=15040` 完整命中。非目标进程中的同名 tag，尤其 sceneboard 的 `OnVsyncEvent now`，没有混入目标链路。
3. 最大阶段是 C `HandleLaunchAbility -> HandleAbilityTransaction`，耗时 483.568 ms，占 fallback 总窗口约 69.9%。
4. C 段关键路径在微信主线程，主导状态是 running，不是 runnable 调度等待、IO wait 或内核阻塞。
5. C 段主要热点是 JS/Ability 初始化与模块加载执行: `JsRuntime::LoadModule/RunScript`、`EntryAbility.abc`、`SourceTextModule::Evaluate EntryAbility`，以及 `aff_biz`、`feat_weapp` 等业务模块。
6. 关键路径小核运行时间约 0.950 ms，占比约 0.153%。当前 trace 不支持“小核是冷启动慢主因”。

## 8. 下一步建议

优先优化 C 段:

- 减少首启同步加载和执行的 `EntryAbility.abc` 代码量。
- 拆分或延后 `aff_biz`、`feat_weapp` 等业务模块初始化。
- 检查 `JsUIAbility::Init` 到 `SourceTextModule::Evaluate` 之间是否有可懒加载、可并行化或可缓存的初始化逻辑。

其次关注 D 段:

- `JsUIAbility::OnStart`、`onCreate`、`UI Initialize:wechat0` 是首帧前 UI 构建开销，但优先级低于 C 段。

如果后续要严格量化输入到首帧:

- 重新采集包含 `touchEventDispatch` 的 trace，或补充输入事件到 `IconStart` 的确定性关联证据。
- 使用设备真实 CPU topology 或频率表确认 CPU cluster 映射。
