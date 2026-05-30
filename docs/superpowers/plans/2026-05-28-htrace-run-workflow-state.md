# htrace Run Workflow State Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a durable run-state workflow layer so OpenCode can resume trace analysis after context compression, enforce stage order, and show user-visible progress.

**Architecture:** Add a focused `run` module containing the state model, fixed workflow rules, progress renderer, and filesystem helpers. Add a `htrace run` command group that exposes `init`, `status`, `guard`, and `advance`, while leaving trace atomic execution and LLM report generation unchanged.

**Tech Stack:** Rust 2021, clap, serde, serde_norway, serde_json, anyhow, chrono, assert_cmd, predicates, tempfile.

---

## File Structure

- Modify `cli/Cargo.toml`: add `chrono` for local timestamp and run id generation.
- Modify `.gitignore`: ignore `/runs/` and `.last-run`.
- Modify `cli/src/lib.rs`: export the new `run` module.
- Modify `cli/src/main.rs`: add the top-level `Run` command.
- Modify `cli/src/commands/mod.rs`: export `commands::run`.
- Create `cli/src/commands/run.rs`: parse CLI flags and call run module APIs.
- Create `cli/src/run/mod.rs`: module exports and public orchestration helpers.
- Create `cli/src/run/model.rs`: serializable state, stage, decision, and CLI summary structs.
- Create `cli/src/run/workflow.rs`: fixed 8-stage order, allowed actions, and transition rules.
- Create `cli/src/run/progress.rs`: render `progress.md` from `RunState`.
- Create `cli/tests/run_cli_test.rs`: CLI integration tests for `init/status/guard/advance`.
- Modify `skill/SKILL.md`: require OpenCode to use `run status/guard/advance` when starting, resuming, and crossing stages.
- Modify `docs/RUST_CLI_ARCHITECTURE.md`: document the new run-state layer.
- Modify `docs/NEXT_ITERATION_HANDOFF.md`: document `runs/`, `.last-run`, and first verification commands.

---

### Task 1: Add Dependencies And Ignore Run Artifacts

**Files:**
- Modify: `cli/Cargo.toml`
- Modify: `.gitignore`

- [ ] **Step 1: Add `chrono` dependency**

Edit `cli/Cargo.toml` dependencies to include:

```toml
chrono = { version = "0.4", features = ["serde"] }
```

The `[dependencies]` block should contain:

```toml
[dependencies]
anyhow = "1"
chrono = { version = "0.4", features = ["serde"] }
clap = { version = "4.5", features = ["derive"] }
rayon = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_norway = "0.9.42"
tempfile = "3"
thiserror = "2"
```

- [ ] **Step 2: Ignore run artifacts**

Edit `.gitignore` to include:

```gitignore
/runs/
.last-run
```

The final file should include:

```gitignore
/target/
/bin/htrace
/bin/htrace.exe
*.pdb
.DS_Store
.last-validation-dir
validation/
/runs/
.last-run
```

- [ ] **Step 3: Verify dependency metadata parses**

Run:

```powershell
cargo metadata --no-deps
```

Expected: command exits `0` and prints workspace JSON containing package `htrace`.

- [ ] **Step 4: Commit**

```powershell
git add cli/Cargo.toml .gitignore
git commit -m "chore: prepare run workflow dependencies"
```

---

### Task 2: Add Run State Model

**Files:**
- Create: `cli/src/run/model.rs`
- Create: `cli/src/run/mod.rs`
- Modify: `cli/src/lib.rs`
- Test: `cli/src/run/model.rs`

- [ ] **Step 1: Write model tests first**

Create `cli/src/run/model.rs` with the tests at the bottom first. The implementation in Step 3 will make them pass.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_starts_at_collect_input() {
        let state = RunState::new(
            "20260528-103012".to_string(),
            "sample.htrace".to_string(),
            "冷启动为什么慢".to_string(),
            Some("scheduler-kernel".to_string()),
            Some("wechat".to_string()),
            "2026-05-28T10:30:12+08:00".to_string(),
        );

        assert_eq!(state.schema_version, 1);
        assert_eq!(state.status, RunStatus::Running);
        assert_eq!(state.current_stage, StageId::CollectInput);
        assert_eq!(state.trace.path, "sample.htrace");
        assert_eq!(state.question.raw, "冷启动为什么慢");
        assert_eq!(state.question.domain_hint.as_deref(), Some("scheduler-kernel"));
        assert_eq!(state.question.target_process_hint.as_deref(), Some("wechat"));
        assert_eq!(
            state.stages.get(&StageId::CollectInput).unwrap().status,
            StageStatus::InProgress
        );
        assert_eq!(
            state.stages.get(&StageId::LoadProfile).unwrap().status,
            StageStatus::Pending
        );
    }

    #[test]
    fn stage_ids_serialize_as_snake_case() {
        let stage = serde_json::to_string(&StageId::OverviewAtomics).unwrap();
        assert_eq!(stage, "\"overview_atomics\"");
    }
}
```

- [ ] **Step 2: Run the model tests and verify they fail**

Run:

```powershell
cargo test -p htrace run::model::tests -- --nocapture
```

Expected: FAIL because `RunState`, `RunStatus`, `StageId`, and `StageStatus` are not defined.

- [ ] **Step 3: Implement the run state model**

Replace `cli/src/run/model.rs` with:

```rust
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum StageId {
    CollectInput,
    LoadProfile,
    OverviewAtomics,
    TopdownBrief,
    StrategySelection,
    DeepAnalysis,
    ReplayGeneration,
    FinalReport,
}

impl StageId {
    pub fn label(&self) -> &'static str {
        match self {
            StageId::CollectInput => "收集输入",
            StageId::LoadProfile => "加载 profile",
            StageId::OverviewAtomics => "执行 overview atomics",
            StageId::TopdownBrief => "编写 Topdown Brief",
            StageId::StrategySelection => "选择或生成策略",
            StageId::DeepAnalysis => "执行深度分析",
            StageId::ReplayGeneration => "生成 replay YAML",
            StageId::FinalReport => "输出最终报告",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Completed,
    Blocked,
    Failed,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StageStatus {
    Pending,
    InProgress,
    Completed,
    Blocked,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct RunState {
    pub schema_version: u32,
    pub run_id: String,
    pub status: RunStatus,
    pub current_stage: StageId,
    pub created_at: String,
    pub updated_at: String,
    pub trace: TraceInput,
    pub question: QuestionInput,
    pub profile: ProfileState,
    pub stages: BTreeMap<StageId, StageState>,
    #[serde(default)]
    pub decisions: Vec<RunDecision>,
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct TraceInput {
    pub path: String,
    pub kind: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct QuestionInput {
    pub raw: String,
    pub domain_hint: Option<String>,
    pub target_process_hint: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProfileState {
    pub selected: Option<String>,
    pub router_result: Option<String>,
    #[serde(default)]
    pub knowledge_loaded: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct StageState {
    pub status: StageStatus,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    #[serde(default)]
    pub artifacts: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct RunDecision {
    pub stage: StageId,
    pub decision: String,
    pub value: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InitSummary {
    pub run_id: String,
    pub run_dir: PathBuf,
    pub state: PathBuf,
    pub progress: PathBuf,
    pub current_stage: StageId,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusSummary {
    pub run_id: String,
    pub status: RunStatus,
    pub current_stage: StageId,
    pub completed_stages: Vec<StageId>,
    pub next_allowed: Vec<String>,
    pub blocked_reason: Option<String>,
    pub progress: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct GuardSummary {
    pub allowed: bool,
    pub current_stage: StageId,
    pub action: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdvanceSummary {
    pub advanced: bool,
    pub from: StageId,
    pub to: StageId,
    pub progress: PathBuf,
}

impl RunState {
    pub fn new(
        run_id: String,
        trace_path: String,
        question: String,
        domain_hint: Option<String>,
        target_process_hint: Option<String>,
        now: String,
    ) -> Self {
        let mut stages = BTreeMap::new();
        for stage in crate::run::workflow::STAGE_ORDER {
            stages.insert(
                stage.clone(),
                StageState {
                    status: if *stage == StageId::CollectInput {
                        StageStatus::InProgress
                    } else {
                        StageStatus::Pending
                    },
                    started_at: if *stage == StageId::CollectInput {
                        Some(now.clone())
                    } else {
                        None
                    },
                    completed_at: None,
                    artifacts: Vec::new(),
                },
            );
        }

        Self {
            schema_version: 1,
            run_id,
            status: RunStatus::Running,
            current_stage: StageId::CollectInput,
            created_at: now.clone(),
            updated_at: now,
            trace: TraceInput {
                path: trace_path,
                kind: "htrace".to_string(),
            },
            question: QuestionInput {
                raw: question,
                domain_hint,
                target_process_hint,
            },
            profile: ProfileState {
                selected: None,
                router_result: None,
                knowledge_loaded: Vec::new(),
            },
            stages,
            decisions: Vec::new(),
            blocked_reason: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_starts_at_collect_input() {
        let state = RunState::new(
            "20260528-103012".to_string(),
            "sample.htrace".to_string(),
            "冷启动为什么慢".to_string(),
            Some("scheduler-kernel".to_string()),
            Some("wechat".to_string()),
            "2026-05-28T10:30:12+08:00".to_string(),
        );

        assert_eq!(state.schema_version, 1);
        assert_eq!(state.status, RunStatus::Running);
        assert_eq!(state.current_stage, StageId::CollectInput);
        assert_eq!(state.trace.path, "sample.htrace");
        assert_eq!(state.question.raw, "冷启动为什么慢");
        assert_eq!(state.question.domain_hint.as_deref(), Some("scheduler-kernel"));
        assert_eq!(state.question.target_process_hint.as_deref(), Some("wechat"));
        assert_eq!(
            state.stages.get(&StageId::CollectInput).unwrap().status,
            StageStatus::InProgress
        );
        assert_eq!(
            state.stages.get(&StageId::LoadProfile).unwrap().status,
            StageStatus::Pending
        );
    }

    #[test]
    fn stage_ids_serialize_as_snake_case() {
        let stage = serde_json::to_string(&StageId::OverviewAtomics).unwrap();
        assert_eq!(stage, "\"overview_atomics\"");
    }
}
```

- [ ] **Step 4: Add module exports**

Create `cli/src/run/mod.rs`:

```rust
pub mod model;
pub mod progress;
pub mod workflow;
```

Modify `cli/src/lib.rs`:

```rust
pub mod commands;
pub mod config;
pub mod engine;
pub mod executor;
pub mod replay;
pub mod run;
```

- [ ] **Step 5: Run tests**

Run:

```powershell
cargo test -p htrace run::model::tests -- --nocapture
```

Expected: PASS, 2 tests passed.

- [ ] **Step 6: Commit**

```powershell
git add cli/src/run/model.rs cli/src/run/mod.rs cli/src/lib.rs
git commit -m "feat: add run state model"
```

---

### Task 3: Add Fixed Workflow Rules

**Files:**
- Create: `cli/src/run/workflow.rs`
- Test: `cli/src/run/workflow.rs`

- [ ] **Step 1: Write workflow tests first**

Create `cli/src/run/workflow.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::model::StageId;

    #[test]
    fn final_report_action_is_denied_during_overview() {
        let result = guard_action(&StageId::OverviewAtomics, "write_final_report");
        assert!(!result.allowed);
        assert_eq!(
            result.reason.as_deref(),
            Some("当前阶段 overview_atomics 不允许动作 write_final_report")
        );
    }

    #[test]
    fn legal_next_stage_is_accepted() {
        assert!(is_valid_transition(&StageId::OverviewAtomics, &StageId::TopdownBrief));
    }

    #[test]
    fn skipped_stage_transition_is_rejected() {
        assert!(!is_valid_transition(&StageId::OverviewAtomics, &StageId::FinalReport));
    }
}
```

- [ ] **Step 2: Run the workflow tests and verify they fail**

Run:

```powershell
cargo test -p htrace run::workflow::tests -- --nocapture
```

Expected: FAIL because `guard_action` and `is_valid_transition` are not defined.

- [ ] **Step 3: Implement workflow rules**

Replace `cli/src/run/workflow.rs` with:

```rust
use crate::run::model::{GuardSummary, StageId};

pub const STAGE_ORDER: &[StageId] = &[
    StageId::CollectInput,
    StageId::LoadProfile,
    StageId::OverviewAtomics,
    StageId::TopdownBrief,
    StageId::StrategySelection,
    StageId::DeepAnalysis,
    StageId::ReplayGeneration,
    StageId::FinalReport,
];

pub fn allowed_actions(stage: &StageId) -> Vec<&'static str> {
    match stage {
        StageId::CollectInput => vec!["complete_input"],
        StageId::LoadProfile => vec!["route_profile", "complete_profile"],
        StageId::OverviewAtomics => vec!["run_overview_atomic", "complete_overview_atomics"],
        StageId::TopdownBrief => vec!["write_topdown_brief", "complete_topdown_brief"],
        StageId::StrategySelection => vec![
            "select_approved_strategy",
            "generate_draft_strategy",
            "request_strategy_review",
            "approve_draft_strategy",
            "complete_strategy_selection",
        ],
        StageId::DeepAnalysis => vec![
            "run_strategy_atomic",
            "branch_strategy",
            "complete_deep_analysis",
        ],
        StageId::ReplayGeneration => vec![
            "write_replay",
            "validate_replay",
            "complete_replay_generation",
        ],
        StageId::FinalReport => vec!["write_final_report", "complete_final_report"],
    }
}

pub fn guard_action(stage: &StageId, action: &str) -> GuardSummary {
    let allowed = allowed_actions(stage).contains(&action);
    GuardSummary {
        allowed,
        current_stage: stage.clone(),
        action: action.to_string(),
        reason: if allowed {
            None
        } else {
            Some(format!(
                "当前阶段 {} 不允许动作 {}",
                stage_name(stage),
                action
            ))
        },
    }
}

pub fn is_valid_transition(from: &StageId, to: &StageId) -> bool {
    STAGE_ORDER
        .windows(2)
        .any(|pair| &pair[0] == from && &pair[1] == to)
}

pub fn stage_name(stage: &StageId) -> &'static str {
    match stage {
        StageId::CollectInput => "collect_input",
        StageId::LoadProfile => "load_profile",
        StageId::OverviewAtomics => "overview_atomics",
        StageId::TopdownBrief => "topdown_brief",
        StageId::StrategySelection => "strategy_selection",
        StageId::DeepAnalysis => "deep_analysis",
        StageId::ReplayGeneration => "replay_generation",
        StageId::FinalReport => "final_report",
    }
}

pub fn completed_before(stage: &StageId) -> Vec<StageId> {
    let mut out = Vec::new();
    for item in STAGE_ORDER {
        if item == stage {
            break;
        }
        out.push(item.clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::model::StageId;

    #[test]
    fn final_report_action_is_denied_during_overview() {
        let result = guard_action(&StageId::OverviewAtomics, "write_final_report");
        assert!(!result.allowed);
        assert_eq!(
            result.reason.as_deref(),
            Some("当前阶段 overview_atomics 不允许动作 write_final_report")
        );
    }

    #[test]
    fn legal_next_stage_is_accepted() {
        assert!(is_valid_transition(&StageId::OverviewAtomics, &StageId::TopdownBrief));
    }

    #[test]
    fn skipped_stage_transition_is_rejected() {
        assert!(!is_valid_transition(&StageId::OverviewAtomics, &StageId::FinalReport));
    }
}
```

- [ ] **Step 4: Run tests**

Run:

```powershell
cargo test -p htrace run::workflow::tests -- --nocapture
```

Expected: PASS, 3 tests passed.

- [ ] **Step 5: Commit**

```powershell
git add cli/src/run/workflow.rs
git commit -m "feat: add run workflow guards"
```

---

### Task 4: Render User-Visible Progress

**Files:**
- Create: `cli/src/run/progress.rs`
- Test: `cli/src/run/progress.rs`

- [ ] **Step 1: Write progress renderer tests first**

Create `cli/src/run/progress.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::model::RunState;

    #[test]
    fn progress_mentions_current_stage_and_next_step() {
        let state = RunState::new(
            "20260528-103012".to_string(),
            "sample.htrace".to_string(),
            "冷启动为什么慢".to_string(),
            Some("scheduler-kernel".to_string()),
            Some("wechat".to_string()),
            "2026-05-28T10:30:12+08:00".to_string(),
        );

        let rendered = render_progress(&state);
        assert!(rendered.contains("Run ID：20260528-103012"));
        assert!(rendered.contains("当前阶段"));
        assert!(rendered.contains("collect_input"));
        assert!(rendered.contains("下一步"));
        assert!(rendered.contains("load_profile"));
    }
}
```

- [ ] **Step 2: Run the progress tests and verify they fail**

Run:

```powershell
cargo test -p htrace run::progress::tests -- --nocapture
```

Expected: FAIL because `render_progress` is not defined.

- [ ] **Step 3: Implement progress rendering**

Replace `cli/src/run/progress.rs` with:

```rust
use crate::run::model::{RunState, StageId, StageStatus};
use crate::run::workflow::{stage_name, STAGE_ORDER};

pub fn render_progress(state: &RunState) -> String {
    let completed = completed_lines(state);
    let next = next_stage(&state.current_stage)
        .map(|stage| format!("{}：{}", stage_name(stage), stage.label()))
        .unwrap_or_else(|| "无，当前流程已到最后阶段".to_string());
    let blocked = state
        .blocked_reason
        .clone()
        .unwrap_or_else(|| "无".to_string());

    format!(
        "# 分析进度\n\nRun ID：{}\nTrace：{}\n问题：{}\n\n## 当前阶段\n\n{}：{}\n\n## 已完成\n\n{}\n\n## 正在进行\n\n{}\n\n## 下一步\n\n{}\n\n## 阻塞项\n\n{}\n\n## 关键产物\n\n- run-state.yaml\n- evidence/\n- artifacts/\n",
        state.run_id,
        state.trace.path,
        state.question.raw,
        stage_name(&state.current_stage),
        state.current_stage.label(),
        completed,
        in_progress_sentence(&state.current_stage),
        next,
        blocked,
    )
}

fn completed_lines(state: &RunState) -> String {
    let lines: Vec<String> = STAGE_ORDER
        .iter()
        .filter_map(|stage| {
            let stage_state = state.stages.get(stage)?;
            if stage_state.status == StageStatus::Completed {
                Some(format!("- {}：{}", stage_name(stage), stage.label()))
            } else {
                None
            }
        })
        .collect();

    if lines.is_empty() {
        "- 无".to_string()
    } else {
        lines.join("\n")
    }
}

fn next_stage(current: &StageId) -> Option<&'static StageId> {
    STAGE_ORDER
        .windows(2)
        .find(|pair| &pair[0] == current)
        .map(|pair| &pair[1])
}

fn in_progress_sentence(stage: &StageId) -> &'static str {
    match stage {
        StageId::CollectInput => "收集 trace 路径、问题、领域提示和目标进程提示。",
        StageId::LoadProfile => "加载 profile、领域知识和 overview atomic 列表。",
        StageId::OverviewAtomics => "运行 overview atomic，确认当前 trace 中实际存在什么异常信号。",
        StageId::TopdownBrief => "基于 overview evidence 总结当前 trace 的问题形态。",
        StageId::StrategySelection => "选择 approved strategy，或生成 draft strategy 请求审核。",
        StageId::DeepAnalysis => "按策略分支执行必要 atomic，并持续落盘 evidence。",
        StageId::ReplayGeneration => "生成只包含确定性步骤的 replay YAML 或 signature YAML。",
        StageId::FinalReport => "输出包含证据链、不确定性和 replay 路径的最终报告。",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::model::RunState;

    #[test]
    fn progress_mentions_current_stage_and_next_step() {
        let state = RunState::new(
            "20260528-103012".to_string(),
            "sample.htrace".to_string(),
            "冷启动为什么慢".to_string(),
            Some("scheduler-kernel".to_string()),
            Some("wechat".to_string()),
            "2026-05-28T10:30:12+08:00".to_string(),
        );

        let rendered = render_progress(&state);
        assert!(rendered.contains("Run ID：20260528-103012"));
        assert!(rendered.contains("当前阶段"));
        assert!(rendered.contains("collect_input"));
        assert!(rendered.contains("下一步"));
        assert!(rendered.contains("load_profile"));
    }
}
```

- [ ] **Step 4: Run tests**

Run:

```powershell
cargo test -p htrace run::progress::tests -- --nocapture
```

Expected: PASS, 1 test passed.

- [ ] **Step 5: Commit**

```powershell
git add cli/src/run/progress.rs
git commit -m "feat: render run progress"
```

---

### Task 5: Add Run Filesystem Operations

**Files:**
- Modify: `cli/src/run/mod.rs`
- Test: `cli/src/run/mod.rs`

- [ ] **Step 1: Write filesystem tests first**

Replace `cli/src/run/mod.rs` with module declarations and tests:

```rust
pub mod model;
pub mod progress;
pub mod workflow;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn init_creates_state_progress_and_last_run() {
        let dir = tempdir().unwrap();
        let out_dir = dir.path().join("runs");
        let summary = init_run(
            &out_dir,
            "sample.htrace",
            "冷启动为什么慢",
            Some("scheduler-kernel".to_string()),
            Some("wechat".to_string()),
        )
        .unwrap();

        assert!(summary.state.exists());
        assert!(summary.progress.exists());
        assert!(summary.run_dir.join("evidence").exists());
        assert!(summary.run_dir.join("artifacts").exists());
        assert!(dir.path().join(".last-run").exists());
    }
}
```

- [ ] **Step 2: Run the filesystem test and verify it fails**

Run:

```powershell
cargo test -p htrace run::tests -- --nocapture
```

Expected: FAIL because `init_run` is not defined.

- [ ] **Step 3: Implement filesystem operations**

Replace `cli/src/run/mod.rs` with:

```rust
pub mod model;
pub mod progress;
pub mod workflow;

use crate::run::model::{
    AdvanceSummary, InitSummary, RunDecision, RunState, StageId, StageStatus, StatusSummary,
};
use crate::run::progress::render_progress;
use crate::run::workflow::{allowed_actions, guard_action, is_valid_transition, STAGE_ORDER};
use anyhow::{bail, Context, Result};
use chrono::Local;
use std::fs;
use std::path::{Path, PathBuf};

pub fn init_run(
    out_dir: &Path,
    trace: &str,
    question: &str,
    domain_hint: Option<String>,
    target_process_hint: Option<String>,
) -> Result<InitSummary> {
    fs::create_dir_all(out_dir).with_context(|| format!("创建 {}", out_dir.display()))?;
    let now = Local::now();
    let run_id = now.format("%Y%m%d-%H%M%S").to_string();
    let timestamp = now.to_rfc3339();
    let run_dir = out_dir.join(&run_id);
    fs::create_dir_all(run_dir.join("evidence"))?;
    fs::create_dir_all(run_dir.join("artifacts"))?;

    let state = RunState::new(
        run_id.clone(),
        trace.to_string(),
        question.to_string(),
        domain_hint,
        target_process_hint,
        timestamp,
    );
    write_state(&run_dir, &state)?;
    write_progress(&run_dir, &state)?;
    fs::write(last_run_path(out_dir), run_dir.display().to_string())?;

    Ok(InitSummary {
        run_id,
        run_dir: run_dir.clone(),
        state: run_dir.join("run-state.yaml"),
        progress: run_dir.join("progress.md"),
        current_stage: StageId::CollectInput,
    })
}

pub fn status_run(run_dir: &Path) -> Result<StatusSummary> {
    let state = read_state(run_dir)?;
    write_progress(run_dir, &state)?;
    let completed_stages = STAGE_ORDER
        .iter()
        .filter_map(|stage| {
            let status = &state.stages.get(stage)?.status;
            if *status == StageStatus::Completed {
                Some(stage.clone())
            } else {
                None
            }
        })
        .collect();
    Ok(StatusSummary {
        run_id: state.run_id,
        status: state.status,
        current_stage: state.current_stage.clone(),
        completed_stages,
        next_allowed: allowed_actions(&state.current_stage)
            .into_iter()
            .map(str::to_string)
            .collect(),
        blocked_reason: state.blocked_reason,
        progress: run_dir.join("progress.md"),
    })
}

pub fn guard_run(run_dir: &Path, action: &str) -> Result<crate::run::model::GuardSummary> {
    let state = read_state(run_dir)?;
    Ok(guard_action(&state.current_stage, action))
}

pub fn advance_run(
    run_dir: &Path,
    from: StageId,
    to: StageId,
    artifacts: Vec<String>,
    decision: Option<String>,
) -> Result<AdvanceSummary> {
    let mut state = read_state(run_dir)?;
    if state.current_stage != from {
        bail!(
            "当前阶段是 {:?}，不能从 {:?} 推进",
            state.current_stage,
            from
        );
    }
    if !is_valid_transition(&from, &to) {
        bail!("非法阶段跳转: {:?} -> {:?}", from, to);
    }

    let now = Local::now().to_rfc3339();
    let from_state = state
        .stages
        .get_mut(&from)
        .with_context(|| format!("缺少阶段 {:?}", from))?;
    from_state.status = StageStatus::Completed;
    from_state.completed_at = Some(now.clone());
    from_state.artifacts.extend(artifacts);

    let to_state = state
        .stages
        .get_mut(&to)
        .with_context(|| format!("缺少阶段 {:?}", to))?;
    to_state.status = StageStatus::InProgress;
    to_state.started_at.get_or_insert(now.clone());

    if let Some(value) = decision {
        state.decisions.push(RunDecision {
            stage: from.clone(),
            decision: "advance".to_string(),
            value,
            reason: format!("阶段 {:?} 完成并进入 {:?}", from, to),
        });
    }

    state.current_stage = to.clone();
    state.updated_at = now;
    state.blocked_reason = None;
    write_state(run_dir, &state)?;
    write_progress(run_dir, &state)?;

    Ok(AdvanceSummary {
        advanced: true,
        from,
        to,
        progress: run_dir.join("progress.md"),
    })
}

fn read_state(run_dir: &Path) -> Result<RunState> {
    let text = fs::read_to_string(run_dir.join("run-state.yaml"))
        .with_context(|| format!("读取 {}", run_dir.join("run-state.yaml").display()))?;
    let state = serde_norway::from_str(&text).context("解析 run-state.yaml")?;
    Ok(state)
}

fn write_state(run_dir: &Path, state: &RunState) -> Result<()> {
    let text = serde_norway::to_string(state).context("序列化 run-state.yaml")?;
    fs::write(run_dir.join("run-state.yaml"), text)
        .with_context(|| format!("写入 {}", run_dir.join("run-state.yaml").display()))
}

fn write_progress(run_dir: &Path, state: &RunState) -> Result<()> {
    fs::write(run_dir.join("progress.md"), render_progress(state))
        .with_context(|| format!("写入 {}", run_dir.join("progress.md").display()))
}

fn last_run_path(out_dir: &Path) -> PathBuf {
    out_dir
        .parent()
        .map(|parent| parent.join(".last-run"))
        .unwrap_or_else(|| PathBuf::from(".last-run"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn init_creates_state_progress_and_last_run() {
        let dir = tempdir().unwrap();
        let out_dir = dir.path().join("runs");
        let summary = init_run(
            &out_dir,
            "sample.htrace",
            "冷启动为什么慢",
            Some("scheduler-kernel".to_string()),
            Some("wechat".to_string()),
        )
        .unwrap();

        assert!(summary.state.exists());
        assert!(summary.progress.exists());
        assert!(summary.run_dir.join("evidence").exists());
        assert!(summary.run_dir.join("artifacts").exists());
        assert!(dir.path().join(".last-run").exists());
    }
}
```

- [ ] **Step 4: Run filesystem tests**

Run:

```powershell
cargo test -p htrace run::tests -- --nocapture
```

Expected: PASS, 1 test passed.

- [ ] **Step 5: Run all run module tests**

Run:

```powershell
cargo test -p htrace run:: -- --nocapture
```

Expected: PASS, all run module tests pass.

- [ ] **Step 6: Commit**

```powershell
git add cli/src/run/mod.rs
git commit -m "feat: persist run state"
```

---

### Task 6: Add `htrace run` CLI Commands

**Files:**
- Create: `cli/src/commands/run.rs`
- Modify: `cli/src/commands/mod.rs`
- Modify: `cli/src/main.rs`
- Test: `cli/tests/run_cli_test.rs`

- [ ] **Step 1: Write CLI integration tests first**

Create `cli/tests/run_cli_test.rs`:

```rust
use assert_cmd::Command;
use predicates::str::contains;
use tempfile::tempdir;

#[test]
fn run_init_creates_state_and_progress() {
    let dir = tempdir().unwrap();
    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args([
        "run",
        "init",
        "--out",
        dir.path().to_str().unwrap(),
        "--trace",
        "sample.htrace",
        "--question",
        "冷启动为什么慢",
        "--domain",
        "scheduler-kernel",
        "--target-process",
        "wechat",
        "--json",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"current_stage\":\"collect_input\""));
}

#[test]
fn run_guard_rejects_skipped_final_report() {
    let dir = tempdir().unwrap();
    let run_dir = create_run(dir.path());

    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args([
        "run",
        "guard",
        run_dir.to_str().unwrap(),
        "--action",
        "write_final_report",
        "--json",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"allowed\":false"))
        .stdout(contains("write_final_report"));
}

fn create_run(out: &std::path::Path) -> std::path::PathBuf {
    let out_dir = out.join("runs");
    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args([
        "run",
        "init",
        "--out",
        out_dir.to_str().unwrap(),
        "--trace",
        "sample.htrace",
        "--question",
        "冷启动为什么慢",
        "--json",
    ]);
    cmd.assert().success();

    let last_run = std::fs::read_to_string(out.join(".last-run")).unwrap();
    std::path::PathBuf::from(last_run)
}
```

- [ ] **Step 2: Run CLI tests and verify they fail**

Run:

```powershell
cargo test -p htrace --test run_cli_test -- --nocapture
```

Expected: FAIL because the `run` top-level command is not implemented.

- [ ] **Step 3: Implement command parser**

Create `cli/src/commands/run.rs`:

```rust
use crate::run::model::StageId;
use crate::run::{advance_run, guard_run, init_run, status_run};
use anyhow::Result;
use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct RunCommand {
    #[command(subcommand)]
    pub action: RunAction,
}

#[derive(Debug, Subcommand)]
pub enum RunAction {
    Init {
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        trace: String,
        #[arg(long)]
        question: String,
        #[arg(long)]
        domain: Option<String>,
        #[arg(long = "target-process")]
        target_process: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Status {
        run_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Guard {
        run_dir: PathBuf,
        #[arg(long)]
        action: String,
        #[arg(long)]
        json: bool,
    },
    Advance {
        run_dir: PathBuf,
        #[arg(long)]
        from: StageId,
        #[arg(long)]
        to: StageId,
        #[arg(long = "artifact")]
        artifacts: Vec<String>,
        #[arg(long)]
        decision: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

pub fn run(cmd: RunCommand) -> Result<()> {
    match cmd.action {
        RunAction::Init {
            out,
            trace,
            question,
            domain,
            target_process,
            json,
        } => {
            let summary = init_run(&out, &trace, &question, domain, target_process)?;
            print_value(&summary, json)?;
        }
        RunAction::Status { run_dir, json } => {
            let summary = status_run(&run_dir)?;
            print_value(&summary, json)?;
        }
        RunAction::Guard {
            run_dir,
            action,
            json,
        } => {
            let summary = guard_run(&run_dir, &action)?;
            print_value(&summary, json)?;
        }
        RunAction::Advance {
            run_dir,
            from,
            to,
            artifacts,
            decision,
            json,
        } => {
            let summary = advance_run(&run_dir, from, to, artifacts, decision)?;
            print_value(&summary, json)?;
        }
    }
    Ok(())
}

fn print_value<T: serde::Serialize + std::fmt::Debug>(value: &T, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string(value)?);
    } else {
        println!("{value:#?}");
    }
    Ok(())
}
```

- [ ] **Step 4: Derive clap value parsing for `StageId`**

Modify the `StageId` derive list in `cli/src/run/model.rs`:

```rust
#[derive(
    Debug,
    Clone,
    Deserialize,
    Serialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    clap::ValueEnum,
)]
#[serde(rename_all = "snake_case")]
pub enum StageId {
```

- [ ] **Step 5: Wire command modules**

Modify `cli/src/commands/mod.rs`:

```rust
pub mod atomic;
pub mod profile;
pub mod replay;
pub mod run;
pub mod strategy;
```

Modify `cli/src/main.rs`:

```rust
use anyhow::Result;
use clap::{Parser, Subcommand};
use htrace::commands::{atomic, profile, replay, run, strategy};

#[derive(Debug, Parser)]
#[command(name = "htrace")]
#[command(about = "面向 OpenCode skill 的鸿蒙 trace 分析运行时")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Version,
    Profile(profile::ProfileCommand),
    Strategy(strategy::StrategyCommand),
    Atomic(atomic::AtomicCommand),
    Replay(replay::ReplayCommand),
    Run(run::RunCommand),
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Version => println!("{}", env!("CARGO_PKG_VERSION")),
        Command::Profile(cmd) => profile::run(cmd)?,
        Command::Strategy(cmd) => strategy::run(cmd)?,
        Command::Atomic(cmd) => atomic::run(cmd)?,
        Command::Replay(cmd) => replay::run(cmd)?,
        Command::Run(cmd) => run::run(cmd)?,
    }
    Ok(())
}
```

- [ ] **Step 6: Run CLI tests**

Run:

```powershell
cargo test -p htrace --test run_cli_test -- --nocapture
```

Expected: PASS, 2 tests passed.

- [ ] **Step 7: Run all tests**

Run:

```powershell
cargo test
```

Expected: PASS, all unit and integration tests pass.

- [ ] **Step 8: Commit**

```powershell
git add cli/src/commands/run.rs cli/src/commands/mod.rs cli/src/main.rs cli/src/run/model.rs cli/tests/run_cli_test.rs
git commit -m "feat: add run workflow cli"
```

---

### Task 7: Test `status` And `advance` Behavior

**Files:**
- Modify: `cli/tests/run_cli_test.rs`

- [ ] **Step 1: Add CLI tests for status and advance**

Append to `cli/tests/run_cli_test.rs`:

```rust
#[test]
fn run_status_reports_current_stage() {
    let dir = tempdir().unwrap();
    let run_dir = create_run(dir.path());

    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args([
        "run",
        "status",
        run_dir.to_str().unwrap(),
        "--json",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"current_stage\":\"collect_input\""))
        .stdout(contains("\"complete_input\""));
}

#[test]
fn run_advance_moves_to_next_stage_and_updates_progress() {
    let dir = tempdir().unwrap();
    let run_dir = create_run(dir.path());

    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args([
        "run",
        "advance",
        run_dir.to_str().unwrap(),
        "--from",
        "collect_input",
        "--to",
        "load_profile",
        "--decision",
        "input collected",
        "--json",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"advanced\":true"))
        .stdout(contains("\"to\":\"load_profile\""));

    let progress = std::fs::read_to_string(run_dir.join("progress.md")).unwrap();
    assert!(progress.contains("load_profile"));
}
```

- [ ] **Step 2: Run the new tests**

Run:

```powershell
cargo test -p htrace --test run_cli_test -- --nocapture
```

Expected: PASS, 4 tests passed.

- [ ] **Step 3: Make CLI enum parsing use snake_case**

Ensure this attribute exists on `StageId` in `cli/src/run/model.rs`:

```rust
#[value(rename_all = "snake_case")]
```

The enum header becomes:

```rust
#[derive(
    Debug,
    Clone,
    Deserialize,
    Serialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    clap::ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum StageId {
```

- [ ] **Step 4: Run tests again**

Run:

```powershell
cargo test -p htrace --test run_cli_test -- --nocapture
```

Expected: PASS, 4 tests passed.

- [ ] **Step 5: Commit**

```powershell
git add cli/src/run/model.rs cli/tests/run_cli_test.rs
git commit -m "test: cover run status and advance"
```

---

### Task 8: Update Skill Instructions For Run-State Guardrails

**Files:**
- Modify: `skill/SKILL.md`
- Test: skill validation command

- [ ] **Step 1: Update `SKILL.md` with resume and guard rules**

Insert this section after `## 操作原则` in `skill/SKILL.md`:

```markdown
## 运行状态硬约束

- 每次开始分析时，先创建或恢复 run：优先使用用户指定 run 目录，其次读取 `.last-run`，否则调用 `htrace run init`。
- 每次用户说“继续”、上下文可能被压缩、或不确定当前阶段时，先调用 `htrace run status`，读取 `progress.md`，再继续。
- 每次执行阶段关键动作前，先调用 `htrace run guard <run-dir> --action <action> --json`；`allowed=false` 时不得执行该动作。
- 每次阶段完成后，调用 `htrace run advance` 推进阶段，并向用户报告当前阶段、已完成、下一步和阻塞项。
- `run-state.yaml` 是当前分析任务的事实源；`progress.md` 是用户可读进度；`validation/` 只用于开发验证，不作为当前 run evidence。
```

Update the `## 标准工作流` list so step 1 starts with run creation:

```markdown
1. 创建或恢复 run，读取 `run-state.yaml` 和 `progress.md`；没有 run 时调用 `htrace run init`。
2. 收集 trace 路径、分析问题、可选领域、进程名和时间范围，并用 `htrace run advance` 完成 `collect_input`。
3. 从 `config/profiles` 加载选定 profile；未指定领域时用 `profile route` 根据问题路由。
4. 在选择深度策略前，先运行 profile 的 overview atomics。
5. 基于当前 trace 的真实证据编写 Topdown Brief；先回答该领域问题在此 trace 中实际存在什么异常信号。
6. 选择 approved strategy；如果没有合适策略，在 `strategies/generated` 下生成 draft strategy 并请求用户审核。
7. 使用 `htrace atomic run` 分步骤执行允许的 atomics；每一步先说明要回答的证据问题，再根据输出决定继续、分支、调参或停止。
8. 生成只包含确定性步骤的 replay YAML；写入 atomic、参数、capture、assertions 和 evidence 路径，不写自然语言推理。
9. 输出包含结论、证据链、分支路径、不确定性、优化建议和 replay 路径的最终报告。
```

Add run commands to `## 命令约定`:

```powershell
htrace run init --out runs --trace <trace> --question "<用户问题>" --domain <domain> --target-process <process> --json
htrace run status <run-dir> --json
htrace run guard <run-dir> --action <action> --json
htrace run advance <run-dir> --from <stage> --to <stage> --decision "<阶段完成说明>" --json
```

- [ ] **Step 2: Validate the skill**

Run:

```powershell
$env:PYTHONUTF8='1'
python C:\Users\77294\.codex\skills\.system\skill-creator\scripts\quick_validate.py D:\work\smartperf\harmony-trace-opencode\skill
```

Expected:

```text
Skill is valid!
```

- [ ] **Step 3: Commit**

```powershell
git add skill/SKILL.md
git commit -m "docs: require run-state guardrails in skill"
```

---

### Task 9: Update Project Documentation

**Files:**
- Modify: `docs/RUST_CLI_ARCHITECTURE.md`
- Modify: `docs/NEXT_ITERATION_HANDOFF.md`
- Modify: `README.md`

- [ ] **Step 1: Update architecture document**

Add this section to `docs/RUST_CLI_ARCHITECTURE.md` after “Replay 层”:

```markdown
### 7. Run 状态层

目录：

- `cli/src/run/`

职责：

- 维护单次分析任务的 `run-state.yaml`。
- 渲染用户可读的 `progress.md`。
- 通过固定 8 阶段工作流提供 `guard` 和 `advance`。
- 支持 OpenCode 在上下文压缩后从 `.last-run` 恢复。

当前命令：

```text
htrace run init
htrace run status
htrace run guard
htrace run advance
```

`run` 层不执行 trace 查询，也不生成报告；它只维护流程状态和阶段合法性。
```

- [ ] **Step 2: Update handoff document**

Add this paragraph to `docs/NEXT_ITERATION_HANDOFF.md` under “当前实现状态”:

```markdown
Run workflow state 已作为下一轮核心能力设计：用户分析任务应写入 `runs/<run-id>/run-state.yaml` 和 `progress.md`，并用 `htrace run status/guard/advance` 恢复和推进阶段。`validation/` 仍只用于开发验证，不能作为当前用户任务状态。
```

- [ ] **Step 3: Update README quick start**

Add this snippet to `README.md` after “快速开始” commands:

```markdown
流程状态命令：

```bash
./target/release/htrace run init --out runs --trace sample.pftrace --question "冷启动为什么慢" --json
./target/release/htrace run status runs/<run-id> --json
./target/release/htrace run guard runs/<run-id> --action write_final_report --json
```
```

- [ ] **Step 4: Run documentation checks**

Run:

```powershell
Select-String -Encoding utf8 -SimpleMatch -Path README.md,docs\RUST_CLI_ARCHITECTURE.md,docs\NEXT_ITERATION_HANDOFF.md -Pattern "htrace run"
```

Expected: output includes all three files.

- [ ] **Step 5: Commit**

```powershell
git add README.md docs/RUST_CLI_ARCHITECTURE.md docs/NEXT_ITERATION_HANDOFF.md
git commit -m "docs: document run workflow state"
```

---

### Task 10: Final Verification And Push

**Files:**
- No code changes unless verification reveals a defect.

- [ ] **Step 1: Run full test suite**

Run:

```powershell
cargo test
```

Expected: PASS, all tests pass.

- [ ] **Step 2: Run manual CLI smoke test**

Run:

```powershell
$root = New-Item -ItemType Directory -Force -Path "$env:TEMP\htrace-run-smoke"
$runs = Join-Path $root.FullName "runs"
.\target\debug\htrace.exe run init --out $runs --trace sample.htrace --question "冷启动为什么慢" --domain scheduler-kernel --target-process wechat --json
$runDir = Get-Content -Raw (Join-Path $root.FullName ".last-run")
.\target\debug\htrace.exe run status $runDir --json
.\target\debug\htrace.exe run guard $runDir --action write_final_report --json
.\target\debug\htrace.exe run advance $runDir --from collect_input --to load_profile --decision "input collected" --json
```

Expected:

- `run init` returns `"current_stage":"collect_input"`.
- `run status` returns `"current_stage":"collect_input"`.
- `run guard write_final_report` returns `"allowed":false`.
- `run advance` returns `"advanced":true` and `"to":"load_profile"`.

- [ ] **Step 3: Validate skill**

Run:

```powershell
$env:PYTHONUTF8='1'
python C:\Users\77294\.codex\skills\.system\skill-creator\scripts\quick_validate.py D:\work\smartperf\harmony-trace-opencode\skill
```

Expected:

```text
Skill is valid!
```

- [ ] **Step 4: Verify ignored artifacts**

Run:

```powershell
git status --ignored --short | Select-String -Pattern 'runs|last-run'
```

Expected: smoke-test run artifacts are ignored if created inside the repository. If smoke output was created under `$env:TEMP`, this command may print nothing.

- [ ] **Step 5: Check git status**

Run:

```powershell
git status --short --branch
```

Expected: clean working tree on `harmony-trace-opencode`.

- [ ] **Step 6: Push branch**

Run:

```powershell
git push origin harmony-trace-opencode
```

Expected: push succeeds and remote branch advances.
