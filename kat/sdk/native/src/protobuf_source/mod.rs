//! 固定 protobuf 生成计划的有界运行时。
//!
//! 本模块只接受构建期生成的关系/列槽位、Arrow Schema 与强类型追加调用；
//! 枚举字段路径只作为定义 relation 的输出值，不用于解释输入数据。

mod buffered_relation;
mod capture;
pub(crate) mod native_hook;
pub(crate) mod profiler_occurrence;
mod row;
mod spec;

pub(crate) use capture::{SourceRelationCapture, SourceRelationLayout};
pub(crate) use row::{BinaryValue, EstimatedRow, EstimatedValue, add_estimated_bytes};
pub(crate) use spec::{BufferOptions, EnumOriginSpec, EnumSymbol, RelationSlot, RelationSpec};
