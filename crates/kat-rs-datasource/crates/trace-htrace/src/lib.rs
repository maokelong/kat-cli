//! htrace parser，将 htrace bytes 解析为 TraceDataset。

mod generated_specs;
mod parser;

pub use generated_specs::table_specs;
pub use parser::{parse, parse_bytes};
