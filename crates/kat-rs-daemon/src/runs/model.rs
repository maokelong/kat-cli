use std::collections::BTreeMap;

use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::api::{
    DatasetDto, RunBriefResponse, RunDetailDto, RunEvidenceResponse, RunStepDto, RunSummaryDto,
};

const FAILED_STATUS: &str = "FAILED";
const PLACEHOLDER_DIAGNOSTIC: &str = "PACK_RUNTIME_NOT_IMPLEMENTED";

#[derive(Debug)]
pub struct RunRecord {
    pub id: String,
    pub pack_ref: String,
    pub dataset: DatasetDto,
    pub inputs: BTreeMap<String, Value>,
    pub status: String,
    pub diagnostic: String,
    pub created_at: OffsetDateTime,
    pub completed_at: Option<OffsetDateTime>,
    pub steps: Vec<RunStepRecord>,
    pub evidence: Vec<Value>,
    pub brief: String,
}

impl RunRecord {
    pub fn failed_placeholder(
        id: String,
        pack_ref: String,
        dataset: DatasetDto,
        inputs: BTreeMap<String, Value>,
    ) -> Self {
        let now = OffsetDateTime::now_utc();
        let diagnostic = PLACEHOLDER_DIAGNOSTIC.to_owned();

        Self {
            id,
            pack_ref,
            dataset,
            inputs,
            status: FAILED_STATUS.to_owned(),
            diagnostic: diagnostic.clone(),
            created_at: now,
            completed_at: Some(now),
            steps: vec![RunStepRecord::failed(
                "pack-runtime",
                "Pack runtime",
                diagnostic.clone(),
            )],
            evidence: Vec::new(),
            brief: format!("{diagnostic}: pack runtime is not implemented yet."),
        }
    }

    pub fn to_summary_dto(&self) -> RunSummaryDto {
        RunSummaryDto {
            id: self.id.clone(),
            pack_ref: self.pack_ref.clone(),
            dataset: self.dataset.clone(),
            status: self.status.clone(),
            diagnostic: self.diagnostic.clone(),
            created_at: format_timestamp(self.created_at),
            completed_at: self.completed_at.map(format_timestamp),
        }
    }

    pub fn to_detail_dto(&self) -> RunDetailDto {
        RunDetailDto {
            summary: self.to_summary_dto(),
            inputs: self.inputs.clone(),
            steps: self.steps.iter().map(RunStepRecord::to_step_dto).collect(),
        }
    }

    pub fn to_evidence_response(&self) -> RunEvidenceResponse {
        RunEvidenceResponse {
            summary: self.to_summary_dto(),
            evidence: self.evidence.clone(),
        }
    }

    pub fn to_brief_response(&self) -> RunBriefResponse {
        RunBriefResponse {
            summary: self.to_summary_dto(),
            brief: self.brief.clone(),
        }
    }
}

#[derive(Debug)]
pub struct RunStepRecord {
    pub id: String,
    pub name: String,
    pub status: String,
    pub diagnostic: String,
}

impl RunStepRecord {
    fn failed(
        id: impl Into<String>,
        name: impl Into<String>,
        diagnostic: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            status: FAILED_STATUS.to_owned(),
            diagnostic: diagnostic.into(),
        }
    }

    fn to_step_dto(&self) -> RunStepDto {
        RunStepDto {
            id: self.id.clone(),
            name: self.name.clone(),
            status: self.status.clone(),
            diagnostic: self.diagnostic.clone(),
        }
    }
}

fn format_timestamp(timestamp: OffsetDateTime) -> String {
    timestamp
        .format(&Rfc3339)
        .expect("UTC timestamp must format as RFC3339")
}
