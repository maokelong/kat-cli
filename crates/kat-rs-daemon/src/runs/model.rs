use serde_json::Value;

use crate::api::{
    DatasetDto, RunBriefResponse, RunDetailDto, RunEvidenceResponse, RunStepDto, RunSummaryDto,
};

const COMPLETED_STATUS: &str = "COMPLETED";

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
    pub fn completed(
        run_id: String,
        pack_ref: String,
        dataset: DatasetDto,
        snapshot_digest: String,
        steps: Vec<RunStepRecord>,
        diagnostics: Vec<Value>,
        evidence: Vec<Value>,
        brief_sections: Vec<Value>,
    ) -> Self {
        Self {
            run_id,
            status: COMPLETED_STATUS.to_owned(),
            pack_ref,
            dataset,
            snapshot_digest,
            steps,
            diagnostics,
            evidence,
            brief_sections,
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
    pub fn completed(
        id: impl Into<String>,
        uses: impl Into<String>,
        output: Option<String>,
        row_count: Option<usize>,
    ) -> Self {
        Self {
            id: id.into(),
            uses: uses.into(),
            status: COMPLETED_STATUS.to_owned(),
            output,
            row_count,
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
