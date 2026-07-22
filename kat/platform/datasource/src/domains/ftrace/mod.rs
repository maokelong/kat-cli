//! ftrace plugin domain decoding.

mod event;
mod packet;

pub(crate) use event::{FtraceCaptureRecord, FtraceEventRecord, FtraceRecord};
pub(crate) use packet::FTRACE_PLUGIN_DECODER;
