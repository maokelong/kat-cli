pub mod model;
pub mod progress;
pub mod workflow;

use crate::run::model::{
    AdvanceSummary, AdvanceTarget, GoSummary, InitSummary, RunDecision, RunFinding, RunState,
    RunStatus, StageId, StageStatus, StatusSummary, ValidateSummary,
};
use crate::run::progress::render_progress;
use crate::run::workflow::{
    allowed_actions, guard_action, is_valid_transition, stage_order, stage_summary,
};
use anyhow::{bail, Context, Result};
use chrono::Local;
use std::fs;
use std::io::ErrorKind;
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
    let base_run_id = now.format("%Y%m%d-%H%M%S-%f").to_string();
    let timestamp = now.to_rfc3339();
    let (run_id, run_dir) = create_unique_run_dir(out_dir, &base_run_id)?;

    fs::create_dir_all(run_dir.join("evidence"))
        .with_context(|| format!("创建 {}", run_dir.join("evidence").display()))?;
    fs::create_dir_all(run_dir.join("artifacts"))
        .with_context(|| format!("创建 {}", run_dir.join("artifacts").display()))?;

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

    let last_run = last_run_path(out_dir);
    fs::write(&last_run, run_dir.display().to_string())
        .with_context(|| format!("写入 {}", last_run.display()))?;

    Ok(InitSummary {
        run_id,
        run_dir: run_dir.clone(),
        state: run_dir.join("run-state.yaml"),
        progress: run_dir.join("progress.md"),
        current_stage: StageId::CollectInput,
    })
}

fn create_unique_run_dir(out_dir: &Path, base_run_id: &str) -> Result<(String, PathBuf)> {
    for attempt in 0..1000 {
        let run_id = if attempt == 0 {
            base_run_id.to_string()
        } else {
            format!("{base_run_id}-{attempt:02}")
        };
        let run_dir = out_dir.join(&run_id);

        match fs::create_dir(&run_dir) {
            Ok(()) => return Ok((run_id, run_dir)),
            Err(err) if err.kind() == ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(err).with_context(|| format!("创建 {}", run_dir.display()));
            }
        }
    }

    bail!("无法生成唯一 run_id: {}", base_run_id)
}

pub fn status_run(run_dir: &Path) -> Result<StatusSummary> {
    let state = read_state(run_dir)?;
    write_progress(run_dir, &state)?;

    let completed_stages = stage_order()
        .iter()
        .filter_map(|stage| {
            let stage_state = state.stages.get(stage)?;
            (stage_state.status == StageStatus::Completed).then(|| stage.clone())
        })
        .collect();
    let next_allowed = if state.status == RunStatus::Completed {
        Vec::new()
    } else {
        allowed_actions(&state.current_stage)
            .into_iter()
            .map(str::to_string)
            .collect()
    };

    Ok(StatusSummary {
        run_id: state.run_id,
        status: state.status,
        current_stage: state.current_stage,
        completed_stages,
        next_allowed,
        blocked_reason: state.blocked_reason,
        progress: run_dir.join("progress.md"),
    })
}

pub fn go_run(run_dir: &Path) -> Result<GoSummary> {
    let state = read_state(run_dir)?;
    write_progress(run_dir, &state)?;

    let findings = validate_go_state(&state);
    let has_error = findings.iter().any(|finding| finding.level == "error");
    let next_action = if state.status == RunStatus::Completed {
        "completed"
    } else if has_error {
        "blocked"
    } else {
        "open_stage"
    };

    Ok(GoSummary {
        run_id: state.run_id,
        status: state.status.clone(),
        current_stage: state.current_stage.clone(),
        next_action: next_action.to_string(),
        stage: if state.status == RunStatus::Completed {
            None
        } else {
            stage_summary(&state.current_stage)
        },
        progress: run_dir.join("progress.md"),
        findings,
    })
}

pub fn validate_run(run_dir: &Path) -> ValidateSummary {
    let state_path = run_dir.join("run-state.yaml");
    if !state_path.exists() {
        return ValidateSummary {
            ok: false,
            run_id: None,
            findings: vec![finding(
                "error",
                "HT001",
                "run-state.yaml",
                "缺少 run-state.yaml。",
            )],
        };
    }

    match read_state(run_dir) {
        Ok(state) => {
            let findings = validate_state(run_dir, &state);
            ValidateSummary {
                ok: !findings.iter().any(|item| item.level == "error"),
                run_id: Some(state.run_id),
                findings,
            }
        }
        Err(err) => ValidateSummary {
            ok: false,
            run_id: None,
            findings: vec![finding(
                "error",
                "HT002",
                "run-state.yaml",
                &format!("无法解析 run-state.yaml：{err}"),
            )],
        },
    }
}

pub fn guard_run(run_dir: &Path, action: &str) -> Result<crate::run::model::GuardSummary> {
    let state = read_state(run_dir)?;
    Ok(guard_action(&state.current_stage, action))
}

pub fn advance_run(
    run_dir: &Path,
    from: StageId,
    to: AdvanceTarget,
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

    if let Some(value) = decision {
        if from == StageId::LoadProfile
            && state.profile.selected.is_none()
            && state.profile.router_result.is_none()
        {
            state.profile.selected = Some(value.clone());
        }

        state.decisions.push(RunDecision {
            stage: from.clone(),
            decision: "advance".to_string(),
            value,
            reason: format!("阶段 {:?} 完成并进入 {:?}", from, to),
        });
    }

    validate_advance_target(run_dir, &state, &from, &to, &artifacts)?;

    let now = Local::now().to_rfc3339();
    let from_state = state
        .stages
        .get_mut(&from)
        .with_context(|| format!("缺少阶段 {:?}", from))?;
    from_state.status = StageStatus::Completed;
    from_state.completed_at = Some(now.clone());
    from_state.artifacts.extend(artifacts);

    if let Some(to_stage) = to.as_stage() {
        let to_state = state
            .stages
            .get_mut(&to_stage)
            .with_context(|| format!("缺少阶段 {:?}", to_stage))?;
        to_state.status = StageStatus::InProgress;
        to_state.started_at.get_or_insert(now.clone());
        state.current_stage = to_stage;
    } else {
        state.status = RunStatus::Completed;
    }

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

fn validate_advance_target(
    run_dir: &Path,
    state: &RunState,
    from: &StageId,
    to: &AdvanceTarget,
    artifacts: &[String],
) -> Result<()> {
    validate_artifacts(run_dir, artifacts)?;
    validate_stage_completion_requirements(run_dir, state, from)?;

    if *to == AdvanceTarget::Completed {
        if *from != StageId::FinalReport {
            bail!("只能从 final_report 推进到 completed");
        }
        return Ok(());
    }

    let to_stage = to
        .as_stage()
        .expect("non-completed advance target must map to a stage");
    if !is_valid_transition(from, &to_stage) {
        bail!("非法阶段跳转: {:?} -> {:?}", from, to_stage);
    }

    Ok(())
}

fn validate_stage_completion_requirements(
    run_dir: &Path,
    state: &RunState,
    from: &StageId,
) -> Result<()> {
    let mut state = state.clone();
    state.current_stage = from.clone();
    let mut findings = Vec::new();
    validate_current_stage_completion(run_dir, &state, &mut findings);
    if let Some(error) = findings
        .into_iter()
        .find(|finding| finding.level == "error")
    {
        bail!("{}", error.message);
    }
    Ok(())
}

fn validate_artifacts(run_dir: &Path, artifacts: &[String]) -> Result<()> {
    for artifact in artifacts {
        if !artifact_exists(run_dir, artifact)? {
            bail!("artifact 不存在: {}", artifact);
        }
    }
    Ok(())
}

fn artifact_exists(run_dir: &Path, artifact: &str) -> Result<bool> {
    let artifact_path = Path::new(artifact);
    if artifact_path.is_absolute() {
        return Ok(artifact_path.exists());
    }

    let cwd = std::env::current_dir().context("读取当前工作目录")?;
    Ok(cwd.join(artifact_path).exists() || run_dir.join(artifact_path).exists())
}

fn validate_go_state(state: &RunState) -> Vec<RunFinding> {
    let mut findings = Vec::new();
    validate_stage_continuity(state, &mut findings);
    if state.current_stage == StageId::OverviewAtomics
        && state.profile.selected.is_none()
        && state.profile.router_result.is_none()
    {
        findings.push(finding(
            "error",
            "HT103",
            "run-state.yaml",
            "overview_atomics 阶段缺少 profile.selected 或 profile.router_result。",
        ));
    }
    findings
}

fn validate_state(run_dir: &Path, state: &RunState) -> Vec<RunFinding> {
    let mut findings = validate_go_state(state);
    validate_current_stage_completion(run_dir, state, &mut findings);
    findings
}

fn validate_stage_continuity(state: &RunState, findings: &mut Vec<RunFinding>) {
    let mut seen_current = false;
    for stage in stage_order() {
        let Some(stage_state) = state.stages.get(stage) else {
            findings.push(finding(
                "error",
                "HT101",
                "run-state.yaml",
                "run-state.yaml 缺少阶段状态。",
            ));
            continue;
        };

        if stage == &state.current_stage {
            seen_current = true;
            continue;
        }

        if !seen_current && stage_state.status == StageStatus::Pending {
            findings.push(finding(
                "error",
                "HT101",
                "run-state.yaml",
                "当前阶段之前存在 pending 阶段。",
            ));
        }

        if seen_current && stage_state.status == StageStatus::Completed {
            findings.push(finding(
                "error",
                "HT101",
                "run-state.yaml",
                "后续阶段不能早于当前阶段完成。",
            ));
        }
    }
}

fn validate_current_stage_completion(
    run_dir: &Path,
    state: &RunState,
    findings: &mut Vec<RunFinding>,
) {
    match state.current_stage {
        StageId::OverviewAtomics => {
            if !has_file_in(&run_dir.join("evidence").join("overview"), &["json", "csv"]) {
                findings.push(finding(
                    "error",
                    "HT201",
                    "evidence/overview",
                    "overview_atomics 阶段缺少 overview evidence。",
                ));
            }
        }
        StageId::TopdownBrief => {
            if !run_dir.join("artifacts").join("topdown-brief.md").exists() {
                findings.push(finding(
                    "error",
                    "HT202",
                    "artifacts/topdown-brief.md",
                    "topdown_brief 阶段缺少 artifacts/topdown-brief.md。",
                ));
            }
        }
        StageId::LoadProfile => {
            if state.profile.selected.is_none() && state.profile.router_result.is_none() {
                findings.push(finding(
                    "error",
                    "HT200",
                    "run-state.yaml",
                    "load_profile 阶段缺少 profile.selected 或 profile.router_result。",
                ));
            }
        }
        StageId::StrategySelection => {
            if !state
                .decisions
                .iter()
                .any(|decision| decision.stage == StageId::StrategySelection)
            {
                findings.push(finding(
                    "error",
                    "HT203",
                    "run-state.yaml",
                    "strategy_selection 阶段缺少策略决策记录。",
                ));
            }
        }
        StageId::DeepAnalysis => {
            if !has_file_in(&run_dir.join("evidence").join("deep"), &["json", "csv"]) {
                findings.push(finding(
                    "error",
                    "HT206",
                    "evidence/deep",
                    "deep_analysis 阶段缺少 deep evidence。",
                ));
            }
        }
        StageId::ReplayGeneration => {
            if !run_dir.join("artifacts").join("replay.yaml").exists()
                && !run_dir.join("artifacts").join("signature.yaml").exists()
            {
                findings.push(finding(
                    "error",
                    "HT204",
                    "artifacts/replay.yaml",
                    "replay_generation 阶段缺少 replay YAML 或 signature YAML。",
                ));
            }
        }
        StageId::FinalReport => {
            if !run_dir.join("artifacts").join("final-report.md").exists() {
                findings.push(finding(
                    "error",
                    "HT205",
                    "artifacts/final-report.md",
                    "final_report 阶段缺少 artifacts/final-report.md。",
                ));
            }
        }
        StageId::CollectInput => {}
    }
}

fn has_file_in(dir: &Path, extensions: &[&str]) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };

    entries.filter_map(|entry| entry.ok()).any(|entry| {
        let path = entry.path();
        path.is_file()
            && path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|extension| extensions.contains(&extension))
    })
}

fn finding(level: &str, code: &str, path: &str, message: &str) -> RunFinding {
    RunFinding {
        level: level.to_string(),
        code: code.to_string(),
        path: path.to_string(),
        message: message.to_string(),
    }
}

fn read_state(run_dir: &Path) -> Result<RunState> {
    let state_path = run_dir.join("run-state.yaml");
    let text = fs::read_to_string(&state_path)
        .with_context(|| format!("读取 {}", state_path.display()))?;
    serde_norway::from_str(&text).context("解析 run-state.yaml")
}

fn write_state(run_dir: &Path, state: &RunState) -> Result<()> {
    let state_path = run_dir.join("run-state.yaml");
    let text = serde_norway::to_string(state).context("序列化 run-state.yaml")?;
    fs::write(&state_path, text).with_context(|| format!("写入 {}", state_path.display()))
}

fn write_progress(run_dir: &Path, state: &RunState) -> Result<()> {
    let progress_path = run_dir.join("progress.md");
    fs::write(&progress_path, render_progress(state))
        .with_context(|| format!("写入 {}", progress_path.display()))
}

fn last_run_path(out_dir: &Path) -> PathBuf {
    // .last-run 与 runs/ 同级，记录最近一次运行目录。
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

    #[test]
    fn init_twice_creates_distinct_runs_and_updates_last_run() {
        let dir = tempdir().unwrap();
        let out_dir = dir.path().join("runs");

        let first = init_run(&out_dir, "sample.htrace", "first question", None, None).unwrap();
        let second = init_run(&out_dir, "sample.htrace", "second question", None, None).unwrap();

        assert_ne!(first.run_id, second.run_id);
        assert_ne!(first.run_dir, second.run_dir);
        assert!(first.state.exists());
        assert!(second.state.exists());
        assert_eq!(
            fs::read_to_string(dir.path().join(".last-run")).unwrap(),
            second.run_dir.display().to_string()
        );
    }
}
