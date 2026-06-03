#![forbid(unsafe_code)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DatasourceCapability {
    pub name: &'static str,
    pub description: &'static str,
}

pub const CAPABILITIES: &[DatasourceCapability] = &[DatasourceCapability {
    name: "trace-datasource",
    description: "Trace datasource library boundary",
}];

pub fn capabilities() -> &'static [DatasourceCapability] {
    CAPABILITIES
}

pub fn crate_boundaries_ready() -> bool {
    trace_parser::parser_shell_ready()
        && trace_query::query_shell_ready()
        && !trace_model::CRATE_ROLE.is_empty()
}

#[cfg(test)]
mod tests {
    use super::{capabilities, crate_boundaries_ready};

    #[test]
    fn exposes_datasource_boundary() {
        assert_eq!(capabilities()[0].name, "trace-datasource");
        assert!(crate_boundaries_ready());
    }
}
