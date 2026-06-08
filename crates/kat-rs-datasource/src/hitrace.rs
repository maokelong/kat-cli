//! Converts the current hitrace protobuf format into an Arrow record batch.

use std::{path::Path, sync::Arc};

use anyhow::{Context, Result};
use arrow_array::{
    RecordBatch,
    builder::{Int32Builder, StringBuilder, UInt64Builder},
};
use arrow_schema::{DataType, Field, Schema};
use log::debug;
use prost::Message;

use crate::{mmap::with_mapped_file, proto::HitraceTrace};

pub(crate) const HITRACE_TABLE: &str = "hitrace_event";

macro_rules! append_hitrace_event {
    ($event:expr, $timestamp:expr, $pid:expr, $tid:expr, $tag:expr, $message:expr) => {{
        $timestamp.append_value($event.timestamp_ns);
        $pid.append_value($event.pid);
        $tid.append_value($event.tid);
        $tag.append_value(&$event.tag);
        $message.append_value(&$event.message);
    }};
}

pub(crate) fn load_hitrace_batch(path: &Path) -> Result<RecordBatch> {
    debug!("building hitrace datasource from {}", path.display());

    let trace = with_mapped_file(path, |bytes| {
        HitraceTrace::decode(bytes).context("failed to decode hitrace protobuf")
    })?;

    let row_count = trace.events.len();
    let schema = Arc::new(Schema::new(vec![
        Field::new("timestamp_ns", DataType::UInt64, false),
        Field::new("pid", DataType::Int32, false),
        Field::new("tid", DataType::Int32, false),
        Field::new("tag", DataType::Utf8, false),
        Field::new("message", DataType::Utf8, false),
    ]));

    let mut timestamp_ns = UInt64Builder::with_capacity(row_count);
    let mut pid = Int32Builder::with_capacity(row_count);
    let mut tid = Int32Builder::with_capacity(row_count);
    let mut tag = StringBuilder::with_capacity(row_count, row_count.saturating_mul(16));
    let mut message = StringBuilder::with_capacity(row_count, row_count.saturating_mul(32));

    for event in &trace.events {
        append_hitrace_event!(event, timestamp_ns, pid, tid, tag, message);
    }

    let batch = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(timestamp_ns.finish()),
            Arc::new(pid.finish()),
            Arc::new(tid.finish()),
            Arc::new(tag.finish()),
            Arc::new(message.finish()),
        ],
    )?;

    debug!("built {row_count} hitrace rows");
    Ok(batch)
}
