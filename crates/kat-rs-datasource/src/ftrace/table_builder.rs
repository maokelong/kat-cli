use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_arrow::{
    ArrayBuilder,
    schema::{SchemaLike, TracingOptions},
};

use crate::proto::kat::hitrace::FtraceEvent;

use super::FtraceTable;

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

    fn arrow_fields() -> Result<Vec<arrow_schema::FieldRef>> {
        Vec::<arrow_schema::FieldRef>::from_type::<Self>(TracingOptions::default())
            .context("failed to trace event metadata Arrow schema")
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

pub(crate) struct DirectEventTableBuilder {
    name: &'static str,
    builder: ArrayBuilder,
}

impl DirectEventTableBuilder {
    pub(crate) fn new<M>(name: &'static str) -> Result<Self>
    where
        for<'de> M: Deserialize<'de>,
    {
        let mut fields = EventMeta::arrow_fields()?;
        fields.extend(Vec::<arrow_schema::FieldRef>::from_type::<M>(
            TracingOptions::default(),
        )?);

        Ok(Self {
            name,
            builder: ArrayBuilder::from_arrow(&fields)?,
        })
    }

    pub(crate) fn push<M>(&mut self, meta: EventMeta, message: M) -> Result<()>
    where
        M: Serialize,
    {
        self.builder.push(EventRow::new(meta, message))?;
        Ok(())
    }

    pub(crate) fn into_table(self) -> Result<FtraceTable> {
        into_table(self.name, self.builder)
    }
}

fn into_table(name: &'static str, builder: ArrayBuilder) -> Result<FtraceTable> {
    Ok(FtraceTable {
        name,
        batches: vec![
            builder
                .into_record_batch()
                .with_context(|| format!("failed to convert {name} table to Arrow"))?,
        ],
    })
}
