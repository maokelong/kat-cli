#![forbid(unsafe_code)]

pub mod manifest;
pub mod schema;
pub mod tables;

pub use manifest::*;
pub use schema::*;
pub use tables::*;

pub const CRATE_ROLE: &str = "trace table model";
