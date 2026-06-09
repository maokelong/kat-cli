//! Converts the current hitrace protobuf format into an Arrow record batch.

use std::path::Path;

use anyhow::{Context, Result};
use arrow_array::RecordBatch;
use log::debug;
use prost::Message;

use crate::{mmap::with_mapped_file, proto::HitraceTrace};

pub(crate) const HITRACE_TABLE: &str = "hitrace_event";

mod generated {
    include!(concat!(
        env!("OUT_DIR"),
        "/hitrace_event_arrow_generated.rs"
    ));
}

use generated::HitraceEventArrowBuilder;

pub(crate) fn load_hitrace_batch(path: &Path) -> Result<RecordBatch> {
    debug!("building hitrace datasource from {}", path.display());

    let trace = with_mapped_file(path, |bytes| {
        HitraceTrace::decode(bytes).context("failed to decode hitrace protobuf")
    })?;

    let row_count = trace.events.len();
    let mut builder = HitraceEventArrowBuilder::with_capacity(row_count);

    for event in &trace.events {
        builder.append(event);
    }

    let batch = builder.finish()?;

    debug!("built {row_count} hitrace rows");
    Ok(batch)
}
