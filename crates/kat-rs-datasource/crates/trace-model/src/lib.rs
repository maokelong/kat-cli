#![forbid(unsafe_code)]

pub mod builders;
pub mod contract;
pub mod tables;

pub use builders::*;
pub use contract::*;
pub use tables::*;

pub const CRATE_ROLE: &str = "trace table model";
