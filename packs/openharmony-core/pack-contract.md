# OpenHarmony Core Pack Contract

本文档描述 `packs/openharmony-core` 当前已有资源，供 LLM 生成或修改 analysis YAML / pack resources 时复用。它不定义 CLI 通用语法；通用语法以 `crates/kat-rs-cli/docs/pack-authoring-contract.md` 为准。

## Pack

- Pack id: `openharmony-core`
- Domain: OpenHarmony trace analysis
- Current primary analysis: `openharmony.critical_path`

## Authoring Rule

LLM 修改本 pack 前必须先读取：

1. `crates/kat-rs-cli/docs/pack-authoring-contract.md`
2. `packs/openharmony-core/pack-contract.md`
3. `packs/openharmony-core/pack.yaml`
4. 相关 `derived`、`queries`、`rules`、`analyses` 文件

生成顺序必须是：

```text
strategy evidence need
  -> kat-rs-cli capability contract
  -> existing OpenHarmony pack resource
  -> minimal pack YAML / SQL change
  -> kat-rs-cli analyze run artifacts
```

`kat-rs-cli analyze` 是必需验证路径；静态 parse/lint/check 只能作为补充，不能替代 run artifacts。当前不要暗示存在独立 `validate` 命令。

## Existing Derived Tables

| Table | File | Kind | Inputs | Schema | Fact Semantics | Reuse Guidance |
| --- | --- | --- | --- | --- | --- | --- |
| `first_draw_window` | `derived/first_draw_window.yaml` | `marker.extract_bracket_fields` | `callstack`, `thread`, `process` | `marker.first_draw_window.v1` | Extracts target process first-draw marker window, root thread, process, vsync id, start/end timestamps. | Use as root seed for first draw analysis. Do not duplicate marker extraction for `firstDrawFrame:1`. |
| `thread_state_segments` | `derived/thread_state_segments.yaml` | `sql.view` | `thread_state` | `thread.state_segments.v1` | Normalizes thread state rows into start/end segments and state classes for one `${itid}`. | Current skill/CLI path cannot directly reuse this resource: the SQL placeholder `${itid}` is a top-level analysis param, while current CLI params only inject `target_process` and `marker`. Do not treat it as root-thread segment facts unless the SQL is changed to use existing state, or CLI param plumbing / analysis parameter injection is added. |
| `thread_identity` | `derived/thread_identity.yaml` | `rules.classify` | `thread` | `thread.identity.v1` | Classifies threads with rules such as `irq_thread` and `io_thread_candidate`. | Reuse before adding new thread classification rules. Currently not consumed by `openharmony.critical_path`. |
| `thread_state_profile` | `derived/thread_state_profile.yaml` | `sql.view` | `thread_state`, `first_draw_window` | `thread.state_profile.v1` | Computes dominant state, dominant percent, running/runnable/sleeping/blocked durations in the first draw window. | Use for high-level state classification of root thread. |
| `callstack_overlap_window` | `derived/callstack_overlap_window.yaml` | `sql.view` | `callstack`, `first_draw_window` | `callstack.overlap_window.v1` | Computes callstack spans overlapping the first draw window. | Reuse before deriving callstack self time or window-level stack facts. |
| `callstack_self_time` | `derived/callstack_self_time.yaml` | `sql.view` | `callstack_overlap_window` | `callstack.self_time.v1` | Computes inclusive/exclusive duration and ranks callstack spans. | Use top span as fact only; do not treat longest span as root cause by itself. |
| `frame_slice_link` | `derived/frame_slice_link.yaml` | `sql.view` | `frame_slice`, `first_draw_window` | `frame.slice_link.v1` | Links app frame slice to downstream render service frame slice and computes downstream duration. | Reuse for app-to-render-service dependency edges. |
| `render_service_context` | `derived/render_service_context.yaml` | `sql.view` | `callstack`, `frame_slice_link` | `render_service.context.v1` | Provides render service callstack/context around linked frame slice. | Reuse with `frame_slice_link` for downstream frame explanation. |
| `io_sample_overlap` | `derived/io_sample_overlap.yaml` | `sql.view` | `first_draw_window`, `file_system_sample`, `bio_latency_sample`, `diskio`, `syscall` | `io.sample_overlap.v1` | Collects IO-related samples overlapping the first draw window. | Resource exists, but `openharmony.critical_path` does not consume it yet. |
| `wakeup_edges` | `derived/wakeup_edges.yaml` | `sql.view` | `instant`, `thread`, `first_draw_window`, `thread_state_profile` | `temporal.wakeup_edges.v1` | Extracts wakeup instant, target itid, waker itid, and dominant root state. | Reuse for sleeping-state wakeup dependency candidates. |

## Existing Queries

| Query | Used By | Purpose |
| --- | --- | --- |
| `queries/thread_state_segments.sql` | `thread_state_segments` | Normalize `thread_state` into start/end segments and state classes for `${itid}`. |
| `queries/thread_state_profile.sql` | `thread_state_profile` | Summarize root-thread state durations inside `first_draw_window`. |
| `queries/callstack_overlap_window.sql` | `callstack_overlap_window` | Select callstack rows overlapping `first_draw_window`. |
| `queries/callstack_self_time.sql` | `callstack_self_time` | Compute callstack inclusive/exclusive ranks from overlap rows. |
| `queries/frame_slice_link.sql` | `frame_slice_link` | Link app frame to render service frame through `frame_slice`. |
| `queries/render_service_context.sql` | `render_service_context` | Add render service callstack context for linked frame. |
| `queries/io_sample_overlap.sql` | `io_sample_overlap` | Collect IO sample rows overlapping first draw window. |
| `queries/wakeup_edges.sql` | `wakeup_edges` | Collect sched wakeup edges for the root window. |

When adding a query, first confirm the transform is `sql.view` in the CLI contract, then ensure every referenced table is listed in `safety.allowedTables`.

## Existing Rules

| File | Top-level Data | Semantics | Reuse Guidance |
| --- | --- | --- | --- |
| `rules/thread_identity.yaml` | `rules` | Classifies `thread.thread_name` into `irq_thread` and `io_thread_candidate`; excludes `hmfs_txn` from IO candidates. | Current `rules.classify` requires exactly one non-empty rules ruleset (`rules`) in the pack. Do not add a second rules file with a non-empty top-level `rules:` block; extend this file only when adding stable thread identity classes consumed by `rules.classify`. |
| `rules/marker_extractors.yaml` | `extractors` | historical/orphaned extractor resource for `first_draw_window`. | Active first draw root uses `derived/first_draw_window.yaml` with `marker.extract_bracket_fields`. A future `payload.extract_fields` consumer would require intentionally changing or adding a matching transform/extractor identity; do not assume this resource is currently wired. |

## Existing Analyses

### `openharmony.critical_path`

File: `analyses/critical-path.plan.yaml`

Inputs:

- `target_process`, required.
- `marker`, default `firstDrawFrame:1`.

Steps:

| Step | Kind | Purpose |
| --- | --- | --- |
| `seed_root` | `evidence.render` | Materializes `first_draw_window` and seeds root facts. |
| `walk_dependencies` | `graph.walk` | Selects dependency candidates through providers. |
| `render_report` | `report.render` | Emits Facts, Inferences, and Uncertainty. |

Graph providers:

| Provider | Input Table | Relation | Semantics |
| --- | --- | --- | --- |
| `self_execution` | `thread_state_profile` | `self_execution` | Selects root self execution when dominant state is `Running` and dominant percent is at least 70. |
| `self_top_span` | `callstack_self_time` | `self_work` | Records top exclusive self-time span as a fact. |
| `sleeping_wakeup` | `wakeup_edges` | `wakeup` | Follows a waker only when `dominant_state == S`, `dominant_percent >= 50`, and a wakeup edge exists. |
| `downstream_frame` | `frame_slice_link` | `frame_downstream` | Follows downstream render service frame when duration is at least 1 ms. |

## Reuse Policy

- New analysis YAML should consume existing derived tables before adding new facts.
- New derived tables must be justified by a strategy evidence need and a CLI transform kind in the authoring contract.
- New SQL must be deterministic pack logic and must satisfy `safety.allowedTables`.
- New rules must be stable classification or extraction data consumed by current CLI primitives.
- Current `openharmony.critical_path` plan should be extended minimally; do not rewrite existing providers unless their semantics change.

## Unwired Resources

- `thread_identity` exists but is not consumed by `openharmony.critical_path`.
- `io_sample_overlap` exists but is not consumed by `openharmony.critical_path`.
- `thread_state_segments` exists but is not consumed by `openharmony.critical_path`; current skill/CLI path cannot directly reuse it because only `target_process` and `marker` are injected as CLI params today. Reuse requires changing the SQL to use existing state, or adding CLI param plumbing / analysis parameter injection for `itid`.

## Current Gaps

- Runnable scheduling details are not represented as facts or providers.
- CPU competition, priority, and affinity are not represented as facts or providers.
- Blocked context inheritance is not represented.
- Binder and lock details are not represented.
- IO / `udk-irq` boundary resources exist partially but are not wired into the critical path graph walk or report.
- `openharmony.critical_path` uses `maxDepth: 3`; the expert strategy default is `max_depth: 8`.
