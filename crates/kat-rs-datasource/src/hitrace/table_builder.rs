use std::marker::PhantomData;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_arrow::{
    ArrayBuilder,
    schema::{SchemaLike, TracingOptions},
};

use crate::proto::kat::hitrace::FtraceEvent;

use super::HitraceTable;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct EventMeta {
    event_timestamp: u64,
    event_cpu: u32,
    event_tgid: i32,
    event_comm: String,
}

impl EventMeta {
    pub(crate) fn from_event(cpu: u32, event: &FtraceEvent) -> Self {
        Self {
            event_timestamp: event.timestamp,
            event_cpu: cpu,
            event_tgid: event.tgid,
            event_comm: event.comm.clone(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct EventRow<M> {
    #[serde(flatten)]
    meta: EventMeta,
    #[serde(flatten)]
    message: M,
}

impl<M> EventRow<M> {
    pub(crate) fn new(meta: EventMeta, message: M) -> Self {
        Self { meta, message }
    }
}

pub(crate) struct TableBuilder<T> {
    name: &'static str,
    builder: ArrayBuilder,
    _row: PhantomData<T>,
}

impl<T> TableBuilder<T> {
    #[allow(dead_code)]
    pub(crate) fn new(name: &'static str) -> Result<Self>
    where
        for<'de> T: Deserialize<'de>,
    {
        let fields = Vec::<arrow_schema::FieldRef>::from_type::<T>(TracingOptions::default())?;
        Self::from_fields(name, &fields)
    }

    pub(crate) fn new_from_sample(name: &'static str) -> Result<Self>
    where
        T: Default + Serialize,
    {
        let sample = [T::default()];
        let fields =
            Vec::<arrow_schema::FieldRef>::from_samples(&sample, TracingOptions::default())?;
        Self::from_fields(name, &fields)
    }

    fn from_fields(name: &'static str, fields: &[arrow_schema::FieldRef]) -> Result<Self> {
        Ok(Self {
            name,
            builder: ArrayBuilder::from_arrow(fields)?,
            _row: PhantomData,
        })
    }

    pub(crate) fn push(&mut self, row: T) -> Result<()>
    where
        T: Serialize,
    {
        self.builder.push(row)?;
        Ok(())
    }

    pub(crate) fn into_table(self) -> Result<HitraceTable> {
        let name = self.name;
        Ok(HitraceTable {
            name,
            batches: vec![
                self.builder
                    .into_record_batch()
                    .with_context(|| format!("failed to convert {name} table to Arrow"))?,
            ],
        })
    }
}
