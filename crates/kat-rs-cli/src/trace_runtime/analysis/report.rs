use anyhow::Result;
use serde_json::Value;

pub fn render_report(state: &Value, evidence: &[Value]) -> Result<String> {
    let mut report = String::new();

    report.push_str("# Facts\n\n");
    render_root_facts(&mut report, state);
    render_evidence_facts(&mut report, evidence);

    report.push_str("\n# Inferences\n\n");
    render_inferences(&mut report, state, evidence);

    report.push_str("\n# Uncertainty\n\n");
    render_uncertainty(&mut report, evidence);

    Ok(report)
}

fn render_root_facts(report: &mut String, state: &Value) {
    let Some(root) = state.get("root") else {
        return;
    };

    push_fact(report, "Target process", root.get("process_name"));
    push_fact(report, "Root itid", root.get("itid"));
    push_fact(report, "Vsync id", root.get("vsync_id"));
    push_fact(report, "Window start", root.get("start_ts"));
    push_fact(report, "Window end", root.get("end_ts"));
}

fn render_evidence_facts(report: &mut String, evidence: &[Value]) {
    for item in evidence {
        let Some(facts) = item.get("facts") else {
            continue;
        };

        if let Some(dominant_state) = facts.get("dominantState").and_then(Value::as_str) {
            match facts.get("dominantPercent").and_then(Value::as_f64) {
                Some(percent) => {
                    report.push_str(&format!(
                        "- Dominant thread state: {dominant_state} ({percent:.2}%)\n"
                    ));
                }
                None => {
                    report.push_str(&format!("- Dominant thread state: {dominant_state}\n"));
                }
            }
        }

        if let Some(top_span_name) = facts.get("topSpanName").and_then(Value::as_str) {
            match facts.get("topSpanDurMs").and_then(Value::as_f64) {
                Some(duration_ms) => {
                    report.push_str(&format!(
                        "- Top self-time span: {top_span_name} ({duration_ms:.2} ms)\n"
                    ));
                }
                None => {
                    report.push_str(&format!("- Top self-time span: {top_span_name}\n"));
                }
            }
        }

        push_fact(report, "IO sample overlap rows", facts.get("overlapRows"));
    }
}

fn render_inferences(report: &mut String, state: &Value, evidence: &[Value]) {
    let mut selected_edges = 0;
    if let Some(decisions) = state.get("decisions").and_then(Value::as_array) {
        for decision in decisions {
            if decision.get("status").and_then(Value::as_str) != Some("selected") {
                continue;
            }

            let Some(relation) = decision.get("relation").and_then(Value::as_str) else {
                continue;
            };

            selected_edges += 1;
            if relation == "self_execution" {
                report.push_str("- Critical path is dominated by root-thread self execution.\n");
            } else {
                report.push_str(&format!(
                    "- Selected critical path edge relation: {relation}.\n"
                ));
            }
        }
    }

    if selected_edges == 0 {
        report.push_str("- Critical path edge is not determined.\n");
    }

    if has_top_span_named(evidence, "CreateImagePixelMap") {
        report.push_str(
            "- Image decode / PixelMap work appears to contribute to the critical path.\n",
        );
    }
}

fn render_uncertainty(report: &mut String, evidence: &[Value]) {
    let mut limitation_count = 0;

    for item in evidence {
        let Some(limitations) = item.get("limitations").and_then(Value::as_array) else {
            continue;
        };

        for limitation in limitations {
            let Some(limitation) = limitation.as_str() else {
                continue;
            };

            limitation_count += 1;
            report.push_str(&format!("- {limitation}\n"));
        }
    }

    if limitation_count == 0 {
        report.push_str("- No explicit limitations were reported by evidence steps.\n");
    }
}

fn has_top_span_named(evidence: &[Value], needle: &str) -> bool {
    evidence.iter().any(|item| {
        item.get("facts")
            .and_then(|facts| facts.get("topSpanName"))
            .and_then(Value::as_str)
            .map(|top_span_name| top_span_name.contains(needle))
            .unwrap_or(false)
    })
}

fn push_fact(report: &mut String, label: &str, value: Option<&Value>) {
    let Some(value) = value else {
        return;
    };

    if let Some(value) = value.as_str() {
        report.push_str(&format!("- {label}: {value}\n"));
    } else if !value.is_null() {
        report.push_str(&format!("- {label}: {value}\n"));
    }
}
