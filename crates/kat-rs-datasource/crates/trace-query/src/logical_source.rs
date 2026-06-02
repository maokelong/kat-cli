use trace_model::ParsedTrace;

#[derive(Debug, Clone)]
pub struct ParsedTraceSource {
    pub dataset_id: String,
    pub source_id: String,
    pub trace_id: String,
    pub parsed: ParsedTrace,
}
