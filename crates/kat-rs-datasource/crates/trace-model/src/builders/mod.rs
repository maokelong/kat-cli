mod batches;
mod rows;
mod table_builder;

pub use rows::*;
pub use table_builder::*;

pub type ModelResult<T> = Result<T, arrow_schema::ArrowError>;
