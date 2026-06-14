use anyhow::{Result, bail};
use serde_json::Value;

pub mod critical_path;
pub mod emit;
pub mod frame;
pub mod inspect;
pub mod thread;
pub mod time;

pub struct OperatorInput<'a> {
    pub name: &'a str,
    pub schema: &'a str,
    pub params: Value,
    pub rows: Vec<Value>,
}

pub fn run_operator(input: OperatorInput<'_>) -> Result<Value> {
    match input.name {
        "emit_rows" => Ok(emit::emit_rows(input.schema, input.rows)),
        "inspect_trace" => inspect::inspect_trace(input.schema, input.rows),
        "resolve_thread_candidates" => thread::resolve_thread_candidates(input.schema, input.rows),
        "classify_thread_identity" => {
            thread::classify_thread_identity(input.schema, input.params, input.rows)
        }
        "extract_first_draw_window" => frame::extract_first_draw_window(input.schema, input.rows),
        "profile_thread_state" => {
            critical_path::profile_thread_state(input.schema, input.params, input.rows)
        }
        "profile_sched_slices" => {
            critical_path::profile_sched_slices(input.schema, input.params, input.rows)
        }
        "profile_callstack_context" => {
            critical_path::profile_callstack_context(input.schema, input.params, input.rows)
        }
        other => bail!("unknown operator `{other}`"),
    }
}
