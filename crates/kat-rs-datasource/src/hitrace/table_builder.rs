use std::marker::PhantomData;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_arrow::{
    ArrayBuilder,
    schema::{SchemaLike, TracingOptions},
};

use super::HitraceTable;

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

    pub(crate) fn into_table(self) -> Result<HitraceTable> {
        Ok(HitraceTable {
            batches: vec![
                self.builder
                    .into_record_batch()
                    .with_context(|| format!("failed to convert {} table to Arrow", self.name))?,
            ],
        })
    }
}
