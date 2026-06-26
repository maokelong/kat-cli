use anyhow::Result;
use serde_json::Value;

use crate::trace_runtime::analysis::report::render_report;

pub fn run_report_render(state: &Value, evidence: &[Value]) -> Result<String> {
    render_report(state, evidence)
}
