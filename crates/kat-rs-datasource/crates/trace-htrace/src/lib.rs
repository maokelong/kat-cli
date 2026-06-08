//! htrace parser，将 htrace bytes 解析为 TraceDataset。

mod parser;
mod table_specs;

pub use parser::{parse, parse_bytes};
pub use table_specs::table_specs;
