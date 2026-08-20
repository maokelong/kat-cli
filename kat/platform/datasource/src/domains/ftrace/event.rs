use crate::proto::kat::hitrace::{ClockDetailMsg, FtraceCpuStatsMsg, FtraceEvent};

#[derive(Clone, Debug)]
pub(crate) enum FtraceCaptureRecord {
    CpuStats(FtraceCpuStatsMsg),
    ClockSnapshot(Vec<ClockDetailMsg>),
    CpuDetail { cpu: u32, overwrite: u64 },
}

#[derive(Clone, Debug)]
pub(crate) enum FtraceRecord {
    Event(Box<FtraceEventRecord>),
}

#[derive(Clone, Debug)]
pub(crate) struct EventContext {
    pub(crate) timestamp: u64,
    pub(crate) cpu: u32,
    pub(crate) tgid: Option<i32>,
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
