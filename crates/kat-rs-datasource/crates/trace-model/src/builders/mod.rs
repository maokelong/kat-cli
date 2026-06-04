mod batch;
mod trace_bounds;

pub use batch::{assemble_trace_table_batch, TraceColumnArray};
pub use trace_bounds::{TraceBoundsBuilder, TraceBoundsRow};

pub type ModelResult<T> = Result<T, arrow_schema::ArrowError>;
