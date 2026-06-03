#![forbid(unsafe_code)]

pub const CRATE_ROLE: &str = "trace parser";

pub fn parser_shell_ready() -> bool {
    !trace_model::CRATE_ROLE.is_empty()
}
