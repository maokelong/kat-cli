use serde_json::{Value, json};

use crate::api::{
    DatasetDto, RunBriefResponse, RunDetailDto, RunEvidenceResponse, RunStepDto, RunSummaryDto,
};

const FAILED_STATUS: &str = "FAILED";
const PLACEHOLDER_DIAGNOSTIC: &str = "PACK_RUNTIME_NOT_IMPLEMENTED";

#[derive(Debug)]
pub struct RunRecord {
    pub run_id: String,
    pub status: String,
    pub pack_ref: String,
    pub dataset: DatasetDto,
    pub snapshot_digest: String,
    pub steps: Vec<RunStepRecord>,
    pub diagnostics: Vec<Value>,
    pub evidence: Vec<Value>,
    pub brief_sections: Vec<Value>,
}

impl RunRecord {
    pub fn failed_placeholder(run_id: String, pack_ref: String, dataset: DatasetDto) -> Self {
        Self {
            run_id,
            status: FAILED_STATUS.to_owned(),
            pack_ref,
            dataset,
            snapshot_digest: PLACEHOLDER_DIAGNOSTIC.to_owned(),
            steps: vec![RunStepRecord::failed("runtime")],
            diagnostics: vec![json!({
                "code": PLACEHOLDER_DIAGNOSTIC,
                "message": "pack runtime is not implemented yet"
            })],
            evidence: Vec::new(),
            brief_sections: Vec::new(),
        }
    }

    pub fn to_summary_dto(&self) -> RunSummaryDto {
        RunSummaryDto {
            run_id: self.run_id.clone(),
            status: self.status.clone(),
            pack_ref: self.pack_ref.clone(),
            dataset: self.dataset.clone(),
            step_count: self.steps.len(),
            evidence_count: self.evidence.len(),
            brief_section_count: self.brief_sections.len(),
        }
    }

    pub fn to_detail_dto(&self) -> RunDetailDto {
        RunDetailDto {
            summary: self.to_summary_dto(),
            steps: self.steps.iter().map(RunStepRecord::to_step_dto).collect(),
            diagnostics: self.diagnostics.clone(),
            snapshot_digest: self.snapshot_digest.clone(),
        }
    }

    pub fn to_evidence_response(&self) -> RunEvidenceResponse {
        RunEvidenceResponse {
            run_id: self.run_id.clone(),
            evidence: self.evidence.clone(),
        }
    }

    pub fn to_brief_response(&self) -> RunBriefResponse {
        RunBriefResponse {
            run_id: self.run_id.clone(),
            sections: self.brief_sections.clone(),
        }
    }
}

#[derive(Debug)]
pub struct RunStepRecord {
    pub id: String,
    pub uses: String,
    pub status: String,
    pub output: Option<String>,
    pub row_count: Option<usize>,
}

impl RunStepRecord {
    fn failed(uses: impl Into<String>) -> Self {
        let uses = uses.into();

        Self {
            id: uses.clone(),
            uses,
            status: FAILED_STATUS.to_owned(),
            output: None,
            row_count: None,
        }
    }

    fn to_step_dto(&self) -> RunStepDto {
        RunStepDto {
            id: self.id.clone(),
            uses: self.uses.clone(),
            status: self.status.clone(),
            output: self.output.clone(),
            row_count: self.row_count,
        }
    }
}
