use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize, ValueEnum, PartialEq, Eq, PartialOrd, Ord)]
#[value(rename_all = "snake_case")]
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

#[derive(Debug, Clone, Deserialize, Serialize, ValueEnum, PartialEq, Eq)]
#[value(rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum AdvanceTarget {
    CollectInput,
    LoadProfile,
    OverviewAtomics,
    TopdownBrief,
    StrategySelection,
    DeepAnalysis,
    ReplayGeneration,
    FinalReport,
    Completed,
}

impl AdvanceTarget {
    pub fn as_stage(&self) -> Option<StageId> {
        match self {
            AdvanceTarget::CollectInput => Some(StageId::CollectInput),
            AdvanceTarget::LoadProfile => Some(StageId::LoadProfile),
            AdvanceTarget::OverviewAtomics => Some(StageId::OverviewAtomics),
            AdvanceTarget::TopdownBrief => Some(StageId::TopdownBrief),
            AdvanceTarget::StrategySelection => Some(StageId::StrategySelection),
            AdvanceTarget::DeepAnalysis => Some(StageId::DeepAnalysis),
            AdvanceTarget::ReplayGeneration => Some(StageId::ReplayGeneration),
            AdvanceTarget::FinalReport => Some(StageId::FinalReport),
            AdvanceTarget::Completed => None,
        }
    }
}

impl From<StageId> for AdvanceTarget {
    fn from(stage: StageId) -> Self {
        match stage {
            StageId::CollectInput => AdvanceTarget::CollectInput,
            StageId::LoadProfile => AdvanceTarget::LoadProfile,
            StageId::OverviewAtomics => AdvanceTarget::OverviewAtomics,
            StageId::TopdownBrief => AdvanceTarget::TopdownBrief,
            StageId::StrategySelection => AdvanceTarget::StrategySelection,
            StageId::DeepAnalysis => AdvanceTarget::DeepAnalysis,
            StageId::ReplayGeneration => AdvanceTarget::ReplayGeneration,
            StageId::FinalReport => AdvanceTarget::FinalReport,
        }
    }
}

impl StageId {
    pub const ORDER: &'static [StageId] = &[
        StageId::CollectInput,
        StageId::LoadProfile,
        StageId::OverviewAtomics,
        StageId::TopdownBrief,
        StageId::StrategySelection,
        StageId::DeepAnalysis,
        StageId::ReplayGeneration,
        StageId::FinalReport,
    ];

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
    pub to: AdvanceTarget,
    pub progress: PathBuf,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StageSummary {
    pub index: usize,
    pub total: usize,
    pub key: StageId,
    pub name: String,
    pub objective: String,
    pub allowed_actions: Vec<String>,
    pub allowed_artifacts: Vec<String>,
    pub required_inputs: Vec<String>,
    pub next_stage: Option<StageId>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RunFinding {
    pub level: String,
    pub code: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GoSummary {
    pub run_id: String,
    pub status: RunStatus,
    pub current_stage: StageId,
    pub next_action: String,
    pub stage: Option<StageSummary>,
    pub progress: PathBuf,
    pub findings: Vec<RunFinding>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ValidateSummary {
    pub ok: bool,
    pub run_id: Option<String>,
    pub findings: Vec<RunFinding>,
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
        for stage in StageId::ORDER {
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
        assert_eq!(
            state.question.domain_hint.as_deref(),
            Some("scheduler-kernel")
        );
        assert_eq!(
            state.question.target_process_hint.as_deref(),
            Some("wechat")
        );
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

    #[test]
    fn new_state_stage_keys_follow_stage_order() {
        let state = RunState::new(
            "20260528-103012".to_string(),
            "sample.htrace".to_string(),
            "why slow".to_string(),
            None,
            None,
            "2026-05-28T10:30:12+08:00".to_string(),
        );

        let stage_keys: Vec<StageId> = state.stages.keys().cloned().collect();
        assert_eq!(stage_keys, StageId::ORDER);
    }

    #[test]
    fn run_state_round_trips_through_yaml() {
        let state = RunState::new(
            "20260528-103012".to_string(),
            "sample.htrace".to_string(),
            "why slow".to_string(),
            Some("scheduler-kernel".to_string()),
            Some("wechat".to_string()),
            "2026-05-28T10:30:12+08:00".to_string(),
        );

        let yaml = serde_norway::to_string(&state).unwrap();
        assert!(yaml.contains("overview_atomics"));

        let restored: RunState = serde_norway::from_str(&yaml).unwrap();
        assert_eq!(restored.current_stage, StageId::CollectInput);
        assert_eq!(restored.stages.len(), 8);
        assert!(restored.stages.contains_key(&StageId::ReplayGeneration));
    }

    #[test]
    fn run_finding_serializes_chinese_message() {
        let finding = RunFinding {
            level: "error".to_string(),
            code: "HT201".to_string(),
            path: "evidence/overview".to_string(),
            message: "overview_atomics 阶段缺少 overview evidence。".to_string(),
        };

        let json = serde_json::to_string(&finding).unwrap();
        assert!(json.contains("\"level\":\"error\""));
        assert!(json.contains("HT201"));
        assert!(json.contains("overview_atomics 阶段缺少 overview evidence。"));
    }

    #[test]
    fn stage_summary_carries_allowed_actions_and_artifacts() {
        let summary = StageSummary {
            index: 3,
            total: 8,
            key: StageId::OverviewAtomics,
            name: "执行 overview atomics".to_string(),
            objective: "运行 profile overview atomics，形成 Topdown Brief 的证据基线。".to_string(),
            allowed_actions: vec![
                "run_overview_atomic".to_string(),
                "complete_overview_atomics".to_string(),
            ],
            allowed_artifacts: vec!["evidence/overview/*.json".to_string()],
            required_inputs: vec!["run-state.yaml".to_string(), "profile.selected".to_string()],
            next_stage: Some(StageId::TopdownBrief),
        };

        assert_eq!(summary.key, StageId::OverviewAtomics);
        assert_eq!(summary.name, "执行 overview atomics");
        assert!(summary
            .allowed_actions
            .contains(&"run_overview_atomic".to_string()));
        assert_eq!(summary.next_stage, Some(StageId::TopdownBrief));
    }
}
