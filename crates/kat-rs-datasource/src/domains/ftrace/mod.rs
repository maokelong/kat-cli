//! ftrace plugin domain decoding.

mod event;
mod packet;

pub(crate) use event::FtraceEventRecord;
pub(crate) use packet::{FTRACE_PLUGIN_NAME, decode_plugin_payload};
