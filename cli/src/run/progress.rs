use crate::run::model::{RunState, StageId, StageStatus};
use crate::run::workflow::{next_stage, stage_name, stage_order};

pub fn render_progress(state: &RunState) -> String {
    let completed = completed_lines(state);
    let in_progress = in_progress_lines(state);
    let next = next_stage(&state.current_stage)
        .map(|stage| format!("{}: {}", stage_name(&stage), stage.label()))
        .unwrap_or_else(|| "无，当前流程已到最后阶段。".to_string());
    let blocked = state.blocked_reason.as_deref().unwrap_or("无").to_string();
    let artifacts = artifact_lines(state);

    format!(
        "# 分析进度 / Progress\n\nRun ID：{}\nTrace：{}\n问题：{}\n\n## 当前阶段 / Current Stage\n\n{}: {}\n\n## 已完成 / Completed\n\n{}\n\n## 正在进行 / In Progress\n\n{}\n\n## 下一步 / Next Step\n\n{}\n\n## 阻塞项 / Blockers\n\n{}\n\n## 关键产物 / Key Artifacts\n\n{}\n",
        state.run_id,
        state.trace.path,
        state.question.raw,
        stage_name(&state.current_stage),
        state.current_stage.label(),
        completed,
        in_progress,
        next,
        blocked,
        artifacts,
    )
}

fn completed_lines(state: &RunState) -> String {
    let lines: Vec<String> = stage_order()
        .iter()
        .filter_map(|stage| {
            let stage_state = state.stages.get(stage)?;
            if stage_state.status == StageStatus::Completed {
                Some(format!("- {}: {}", stage_name(stage), stage.label()))
            } else {
                None
            }
        })
        .collect();

    non_empty_lines(lines)
}

fn in_progress_lines(state: &RunState) -> String {
    let lines: Vec<String> = stage_order()
        .iter()
        .filter_map(|stage| {
            let stage_state = state.stages.get(stage)?;
            if stage_state.status == StageStatus::InProgress {
                Some(format!(
                    "- {}: {} - {}",
                    stage_name(stage),
                    stage.label(),
                    in_progress_sentence(stage)
                ))
            } else {
                None
            }
        })
        .collect();

    non_empty_lines(lines)
}

fn artifact_lines(state: &RunState) -> String {
    let mut lines = vec![
        "- run-state.yaml".to_string(),
        "- progress.md".to_string(),
        "- evidence/".to_string(),
        "- artifacts/".to_string(),
    ];

    for stage in stage_order() {
        if let Some(stage_state) = state.stages.get(stage) {
            for artifact in &stage_state.artifacts {
                lines.push(format!("- {}: {}", stage_name(stage), artifact));
            }
        }
    }

    lines.join("\n")
}

fn in_progress_sentence(stage: &StageId) -> &'static str {
    match stage {
        StageId::CollectInput => "收集 trace 路径、问题、领域提示和目标进程提示。",
        StageId::LoadProfile => "加载 profile、领域知识和 overview atomic 列表。",
        StageId::OverviewAtomics => "运行 overview atomics，并识别有 trace 证据支持的信号。",
        StageId::TopdownBrief => "基于 overview 证据概括问题形态。",
        StageId::StrategySelection => "选择已批准的策略，或起草策略供审查。",
        StageId::DeepAnalysis => "执行策略 atomics，并持久化证据。",
        StageId::ReplayGeneration => "生成确定性的 replay YAML 或 signature YAML。",
        StageId::FinalReport => "撰写包含证据、不确定性和 replay 路径的最终报告。",
    }
}

fn non_empty_lines(lines: Vec<String>) -> String {
    if lines.is_empty() {
        "- 无".to_string()
    } else {
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::model::{RunState, StageId, StageStatus};

    #[test]
    fn progress_mentions_current_stage_and_next_step() {
        let state = RunState::new(
            "20260528-103012".to_string(),
            "sample.htrace".to_string(),
            "cold start is slow".to_string(),
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
        assert!(rendered.contains("收集 trace 路径"));
        assert!(rendered.contains("无"));
    }

    #[test]
    fn progress_lists_completed_collect_input_when_current_stage_is_load_profile() {
        let mut state = RunState::new(
            "20260528-103012".to_string(),
            "sample.htrace".to_string(),
            "cold start is slow".to_string(),
            None,
            None,
            "2026-05-28T10:30:12+08:00".to_string(),
        );
        state.current_stage = StageId::LoadProfile;
        state.stages.get_mut(&StageId::CollectInput).unwrap().status = StageStatus::Completed;
        state.stages.get_mut(&StageId::LoadProfile).unwrap().status = StageStatus::InProgress;

        let rendered = render_progress(&state);
        assert!(rendered.contains("已完成"));
        assert!(rendered.contains("- collect_input"));
        assert!(rendered.contains("当前阶段"));
        assert!(rendered.contains("load_profile"));
        assert!(rendered.contains("加载 profile"));
    }
}
