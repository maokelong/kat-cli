use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{
    arrow_table::ArrowTable,
    domains::fixed_result::{FixedResultChildMeta, FixedResultMessage, ProfilerEnvelopeMeta},
};

use super::table::EventTableBuilder;

pub(crate) struct FixedResultMessageTableBuilder {
    builder: EventTableBuilder<ProfilerEnvelopeMeta>,
}

impl FixedResultMessageTableBuilder {
    pub(crate) fn new<M>(name: &'static str) -> Result<Self>
    where
        for<'de> M: Deserialize<'de>,
    {
        Ok(Self {
            builder: EventTableBuilder::new::<M>(name)?,
        })
    }

    pub(crate) fn push<M>(&mut self, record: FixedResultMessage<M>) -> Result<()>
    where
        M: Serialize,
    {
        self.builder.push(record.meta, record.message)
    }

    pub(crate) fn flush_table(&mut self) -> Result<ArrowTable> {
        self.builder.flush_table()
    }

    pub(crate) fn into_table(self) -> Result<ArrowTable> {
        self.builder.into_table()
    }
}

pub(crate) struct FixedResultChildTableBuilder {
    builder: EventTableBuilder<FixedResultChildMeta>,
}

impl FixedResultChildTableBuilder {
    pub(crate) fn new<M>(name: &'static str) -> Result<Self>
    where
        for<'de> M: Deserialize<'de>,
    {
        Ok(Self {
            builder: EventTableBuilder::new::<M>(name)?,
        })
    }

    pub(crate) fn push<M>(&mut self, meta: FixedResultChildMeta, message: M) -> Result<()>
    where
        M: Serialize,
    {
        self.builder.push(meta, message)
    }

    pub(crate) fn flush_table(&mut self) -> Result<ArrowTable> {
        self.builder.flush_table()
    }

    pub(crate) fn into_table(self) -> Result<ArrowTable> {
        self.builder.into_table()
    }
}
