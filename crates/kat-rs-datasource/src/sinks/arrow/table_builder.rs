use std::marker::PhantomData;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_arrow::{
    ArrayBuilder,
    schema::{SchemaLike, TracingOptions},
};

use crate::{
    catalog::{TableCategory, TraceTable},
    domains::ftrace::FtraceEventRecord,
};

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

    pub(crate) fn into_table(self) -> Result<TraceTable> {
        into_table(self.name, TableCategory::DirectEvent, self.builder)
    }
}

pub(crate) struct TableBuilder<T> {
    name: &'static str,
    builder: ArrayBuilder,
    _row: PhantomData<T>,
}

impl<T> TableBuilder<T> {
    pub(crate) fn new(name: &'static str) -> Result<Self>
    where
        for<'de> T: Deserialize<'de>,
    {
        let fields = Vec::<arrow_schema::FieldRef>::from_type::<T>(TracingOptions::default())?;
        Ok(Self {
            name,
            builder: ArrayBuilder::from_arrow(&fields)?,
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

    pub(crate) fn into_table(self, category: TableCategory) -> Result<TraceTable> {
        into_table(self.name, category, self.builder)
    }
}

fn into_table(
    name: &'static str,
    category: TableCategory,
    builder: ArrayBuilder,
) -> Result<TraceTable> {
    Ok(TraceTable::new(
        name,
        category,
        vec![
            builder
                .into_record_batch()
                .with_context(|| format!("failed to convert {name} table to Arrow"))?,
        ],
    ))
}
