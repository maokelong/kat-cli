//! 固定 protobuf 生成计划的有界运行时。
//!
//! 本模块只接受构建期生成的关系/列槽位、Arrow Schema 与强类型追加调用；
//! 枚举字段路径只作为定义表的输出值，不用于解释输入数据。

mod capture;
pub(crate) mod native_hook;
pub(crate) mod profiler_occurrence;
mod row;
mod spec;
mod spool;

#[cfg(all(test, not(doctest)))]
#[path = "../../tests/native_hook_source_capture_contract/mod.rs"]
mod native_hook_contract_tests;

pub(crate) use capture::{SourceTableCapture, SourceTableLayout};
pub(crate) use row::{BinaryValue, EstimatedRow, EstimatedValue, add_estimated_bytes};
pub(crate) use spec::{EnumOriginSpec, EnumSymbol, RelationSlot, RelationSpec, SpoolOptions};
pub(crate) use spool::PreparedSourceTables;
