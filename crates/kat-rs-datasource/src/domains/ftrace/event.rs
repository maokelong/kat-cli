// ftrace 事件记录保留事件公共上下文和原始事件体，供后续 sink 决定物化方式。
use crate::proto::kat::hitrace::FtraceEvent;

#[derive(Clone, Debug)]
pub(crate) struct EventContext {
    pub(crate) timestamp: u64,
    pub(crate) cpu: u32,
    pub(crate) tgid: i32,
    pub(crate) comm: String,
}

impl EventContext {
    pub(crate) fn from_event(cpu: u32, event: &FtraceEvent) -> Self {
        Self {
            timestamp: event.timestamp,
            cpu,
            tgid: event.tgid,
            comm: event.comm.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct FtraceEventRecord {
    pub(crate) context: EventContext,
    pub(crate) event: FtraceEvent,
}

impl FtraceEventRecord {
    pub(crate) fn new(cpu: u32, event: FtraceEvent) -> Self {
        Self {
            context: EventContext::from_event(cpu, &event),
            event,
        }
    }
}
