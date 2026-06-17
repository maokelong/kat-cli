use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{
    catalog::{TableCategory, TraceTable},
    domains::native_hook::NativeHookEvent,
};

use super::table::EventTableBuilder;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct NativeHookEventMeta {
    tv_sec: u64,
    tv_nsec: u64,
}

impl NativeHookEventMeta {
    pub(crate) fn from_record<T>(record: &NativeHookEvent<T>) -> Self {
        Self {
            tv_sec: record.context.tv_sec,
            tv_nsec: record.context.tv_nsec,
        }
    }
}

pub(crate) struct NativeHookEventTableBuilder {
    builder: EventTableBuilder<NativeHookEventMeta>,
}

impl NativeHookEventTableBuilder {
    pub(crate) fn new<M>(name: &'static str) -> Result<Self>
    where
        for<'de> M: Deserialize<'de>,
    {
        Ok(Self {
            builder: EventTableBuilder::new::<M>(name)?,
        })
    }

    pub(crate) fn push<M>(&mut self, record: NativeHookEvent<M>) -> Result<()>
    where
        M: Serialize,
    {
        let meta = NativeHookEventMeta::from_record(&record);
        self.builder.push(meta, record.event)?;
        Ok(())
    }

    pub(crate) fn into_table(self, category: TableCategory) -> Result<TraceTable> {
        self.builder.into_table(category)
    }
}
