//! trace_arrow 通用 Arrow 数据结构、构建期契约和运行时转换。

mod common;
mod contract;
mod runtime;

pub use common::{ArrowTable, TraceDataset};
pub use contract::{schema_for_table, ArrowType, FieldSpec, TableSpec};
pub use runtime::build_table;
