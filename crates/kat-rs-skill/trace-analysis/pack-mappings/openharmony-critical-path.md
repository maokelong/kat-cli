# OpenHarmony Critical Path Pack Mapping

本文说明 `strategies/critical-path.strategy.md` 如何映射到 `packs/openharmony-core`。它是 `kat-rs-skill` 的审阅辅助文档，不是 `kat-rs-cli` 执行 source of truth。

## Pack

- Pack root: `packs/openharmony-core`
- Analysis id: `openharmony.critical_path`
- Primary CLI: `cargo run -p kat-rs-cli -- analyze`

## Status Model

- `covered`：pack resource、analysis step、evidence/report 都已接上。
- `partial`：只覆盖部分事实或部分判断。
- `unwired`：pack resource 已存在，但当前 analysis 没有消费或报告没有使用。
- `missing`：当前 pack / CLI 完全无法表达。

## Coverage Matrix

| Strategy item | Pack resource | Analysis step | CLI artifact | Review point | Status | Gap |
| --- | --- | --- | --- | --- | --- | --- |
| first draw root | `derived/first_draw_window.yaml` | `seed_root` | `seed_root` evidence, `state.json.root` | root 是否有 process、itid、start_ts、end_ts、vsync_id | `covered` | 无 |
| thread state | `derived/thread_state_profile.yaml` | `self_execution` provider | graph evidence, `state.json.decisions` | dominant state 和 percent 是否支撑自身执行判断 | `covered` | 无 |
| self work | `derived/callstack_self_time.yaml` | `self_top_span` provider | graph evidence annotations | top span 只能作为事实，不单独作为根因 | `covered` | 无 |
| wakeup dependency | `derived/wakeup_edges.yaml` | `sleeping_wakeup` provider | graph decision relation `wakeup` | sleeping dominant state 时是否能找到 waker | `partial` | 只覆盖 sleeping dominant state 且存在 wakeup edge 的场景 |
| depth limit | `analyses/critical-path.plan.yaml` | `walk_dependencies.limits.maxDepth` | `plan.json`, run review uncertainty | strategy 默认 `max_depth` 是 8，当前 plan 使用 `maxDepth: 3` | `partial` | 更深依赖链会被截断 |
| downstream frame | `derived/frame_slice_link.yaml`, `derived/render_service_context.yaml` | `downstream_frame` provider | graph decision relation `frame_downstream` | 是否说明 app 到 render service 的下游关系 | `covered` | 无 |
| report contract | `report.render` | `render_report` | `report.md` | Facts、Inferences、Uncertainty 是否分离 | `covered` | 无 |
| IO and udk-irq boundary | `derived/thread_identity.yaml`, `rules/thread_identity.yaml`, `derived/io_sample_overlap.yaml` | none in `openharmony.critical_path` | none | IO thread set、udk-irq stop boundary 已有部分资源但当前 analysis 未消费 | `unwired` | 需要 analysis provider 或 report support |
| runnable scheduling | none | none | none | CPU competition、priority、affinity 不应被伪造 | `missing` | 需要新的事实资源和 analysis provider |
| blocked context inheritance | none | none | none | blocked function context 不应被伪造 | `missing` | 需要 blocked context 事实资源、继承规则和 evidence 输出 |
| Binder / lock details | none | none | none | Binder、锁细节不应被伪造 | `missing` | 需要对应 pack resources 和 report support |

## Run Review Notes

运行命令由用户输入和 `orchestrators/cli-run-review.protocol.md` 生成，不写死在 mapping 文档中。

CLI 生成的 `checklist.md` 可以作为 artifact 审阅；本 mapping 不再维护 strategy-specific checklist。mapping 中的 `partial`、`unwired` 和 `missing` 项必须进入 run review 的 uncertainty 或 gaps，不能写成已验证事实。
