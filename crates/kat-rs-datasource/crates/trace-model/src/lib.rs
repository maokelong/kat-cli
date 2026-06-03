#![forbid(unsafe_code)]

pub mod schema;
pub mod tables;

pub use schema::*;
pub use tables::*;

pub const CRATE_ROLE: &str = "trace table model";
