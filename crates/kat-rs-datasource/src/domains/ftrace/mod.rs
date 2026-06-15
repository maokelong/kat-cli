// ftrace domain 只负责插件载荷到中立事件记录的转换，不直接生成查询表。

mod event;
mod packet;

pub(crate) use event::FtraceEventRecord;
pub(crate) use packet::FTRACE_PLUGIN_DECODER;
