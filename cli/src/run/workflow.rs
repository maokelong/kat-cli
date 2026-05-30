use crate::run::model::{GuardSummary, StageId, StageSummary};

pub fn stage_order() -> &'static [StageId] {
    StageId::ORDER
}

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
    StageId::ORDER
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
    let mut completed = Vec::new();
    for item in StageId::ORDER {
        if item == stage {
            break;
        }
        completed.push(item.clone());
    }
    completed
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
        assert!(is_valid_transition(
            &StageId::OverviewAtomics,
            &StageId::TopdownBrief
        ));
    }

    #[test]
    fn skipped_stage_transition_is_rejected() {
        assert!(!is_valid_transition(
            &StageId::OverviewAtomics,
            &StageId::FinalReport
        ));
    }

    #[test]
    fn completed_before_topdown_brief_returns_prior_stages() {
        assert_eq!(
            completed_before(&StageId::TopdownBrief),
            vec![
                StageId::CollectInput,
                StageId::LoadProfile,
                StageId::OverviewAtomics
            ]
        );
    }

    #[test]
    fn overview_stage_summary_exposes_chinese_metadata() {
        let summary = stage_summary(&StageId::OverviewAtomics).unwrap();

        assert_eq!(summary.index, 3);
        assert_eq!(summary.total, 8);
        assert_eq!(summary.name, "执行 overview atomics");
        assert!(summary
            .allowed_actions
            .contains(&"run_overview_atomic".to_string()));
        assert!(summary
            .allowed_artifacts
            .contains(&"evidence/overview/*.json".to_string()));
        assert!(summary
            .required_inputs
            .contains(&"profile.selected".to_string()));
        assert_eq!(summary.next_stage, Some(StageId::TopdownBrief));
    }

    #[test]
    fn final_report_stage_summary_has_no_next_stage() {
        let summary = stage_summary(&StageId::FinalReport).unwrap();
        assert_eq!(summary.index, 8);
        assert_eq!(summary.next_stage, None);
        assert!(summary
            .allowed_artifacts
            .contains(&"artifacts/final-report.md".to_string()));
    }
}

pub fn stage_summary(stage: &StageId) -> Option<StageSummary> {
    let index = StageId::ORDER.iter().position(|item| item == stage)? + 1;
    let total = StageId::ORDER.len();
    Some(StageSummary {
        index,
        total,
        key: stage.clone(),
        name: stage.label().to_string(),
        objective: stage_objective(stage).to_string(),
        allowed_actions: allowed_actions(stage)
            .into_iter()
            .map(str::to_string)
            .collect(),
        allowed_artifacts: allowed_artifacts(stage)
            .into_iter()
            .map(str::to_string)
            .collect(),
        required_inputs: required_inputs(stage)
            .into_iter()
            .map(str::to_string)
            .collect(),
        next_stage: next_stage(stage),
    })
}

pub fn next_stage(stage: &StageId) -> Option<StageId> {
    StageId::ORDER
        .windows(2)
        .find_map(|pair| (&pair[0] == stage).then(|| pair[1].clone()))
}

pub fn allowed_artifacts(stage: &StageId) -> Vec<&'static str> {
    match stage {
        StageId::CollectInput => vec!["run-state.yaml", "progress.md"],
        StageId::LoadProfile => vec!["run-state.yaml", "progress.md"],
        StageId::OverviewAtomics => vec![
            "evidence/overview/*.json",
            "evidence/overview/*.csv",
            "evidence/overview/*.stderr.txt",
        ],
        StageId::TopdownBrief => vec!["artifacts/topdown-brief.md"],
        StageId::StrategySelection => vec![
            "artifacts/strategy-selection.md",
            "artifacts/draft-strategy.md",
            "run-state.yaml",
        ],
        StageId::DeepAnalysis => vec![
            "evidence/deep/*.json",
            "evidence/deep/*.csv",
            "evidence/deep/*.stderr.txt",
        ],
        StageId::ReplayGeneration => vec!["artifacts/replay.yaml", "artifacts/signature.yaml"],
        StageId::FinalReport => vec!["artifacts/final-report.md"],
    }
}

pub fn required_inputs(stage: &StageId) -> Vec<&'static str> {
    match stage {
        StageId::CollectInput => vec!["trace.path", "question.raw"],
        StageId::LoadProfile => vec!["run-state.yaml", "config/profiles/*.yaml"],
        StageId::OverviewAtomics => vec!["run-state.yaml", "profile.selected"],
        StageId::TopdownBrief => vec!["evidence/overview"],
        StageId::StrategySelection => vec!["artifacts/topdown-brief.md"],
        StageId::DeepAnalysis => vec!["strategy decision"],
        StageId::ReplayGeneration => vec!["evidence/deep"],
        StageId::FinalReport => vec!["artifacts/replay.yaml", "evidence"],
    }
}

fn stage_objective(stage: &StageId) -> &'static str {
    match stage {
        StageId::CollectInput => "确认 trace、问题、领域和目标进程，为当前分析创建事实源。",
        StageId::LoadProfile => "选择或路由 profile，并加载当前领域所需知识。",
        StageId::OverviewAtomics => {
            "运行 profile overview atomics，形成 Topdown Brief 的证据基线。"
        }
        StageId::TopdownBrief => "基于当前 trace 的 overview evidence 编写 Topdown Brief。",
        StageId::StrategySelection => {
            "选择 approved strategy，或生成并等待用户审核 draft strategy。"
        }
        StageId::DeepAnalysis => "按策略执行深度 atomic，记录分支判断和关键 evidence。",
        StageId::ReplayGeneration => "生成只包含确定性步骤的 replay YAML 或 signature YAML。",
        StageId::FinalReport => "输出引用 evidence 和 replay 的最终中文分析报告。",
    }
}
