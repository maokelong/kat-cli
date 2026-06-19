use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{arrow_table::ArrowTable, domains::ftrace::FtraceEventRecord};

use super::table::EventTableBuilder;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct EventMeta {
    event_timestamp: u64,
    event_cpu: u32,
    event_tgid: i32,
    event_comm: String,
}

impl EventMeta {
    pub(crate) fn from_record(record: &FtraceEventRecord) -> Self {
        Self {
            event_timestamp: record.context.timestamp,
            event_cpu: record.context.cpu,
            event_tgid: record.context.tgid,
            event_comm: record.context.comm.clone(),
        }
    }
}

pub(crate) struct FtraceEventTableBuilder {
    builder: EventTableBuilder<EventMeta>,
}

impl FtraceEventTableBuilder {
    pub(crate) fn new<M>(name: &'static str) -> Result<Self>
    where
        for<'de> M: Deserialize<'de>,
    {
        Ok(Self {
            builder: EventTableBuilder::new::<M>(name)?,
        })
    }

    pub(crate) fn push<M>(&mut self, meta: EventMeta, message: M) -> Result<()>
    where
        M: Serialize,
    {
        self.builder.push(meta, message)?;
        Ok(())
    }

    pub(crate) fn into_table(self) -> Result<ArrowTable> {
        self.builder.into_table()
    }
}
