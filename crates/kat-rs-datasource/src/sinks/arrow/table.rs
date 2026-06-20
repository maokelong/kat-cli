use std::marker::PhantomData;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_arrow::{
    ArrayBuilder,
    schema::{SchemaLike, TracingOptions},
};

use crate::arrow_table::ArrowTable;

pub(crate) struct MessageTableBuilder<T> {
    name: &'static str,
    builder: ArrayBuilder,
    _row: PhantomData<T>,
}

impl<T> MessageTableBuilder<T> {
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

    pub(crate) fn flush_table(&mut self) -> Result<ArrowTable> {
        let batch = self
            .builder
            .to_record_batch()
            .with_context(|| format!("failed to flush {} table to Arrow", self.name))?;
        Ok(ArrowTable::new(self.name, vec![batch]))
    }

    pub(crate) fn into_table(self) -> Result<ArrowTable> {
        into_table(self.name, self.builder)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct EventTableRow<Meta, M> {
    #[serde(flatten)]
    meta: Meta,
    #[serde(flatten)]
    message: M,
}

impl<Meta, M> EventTableRow<Meta, M> {
    fn new(meta: Meta, message: M) -> Self {
        Self { meta, message }
    }
}

pub(crate) struct EventTableBuilder<Meta> {
    name: &'static str,
    builder: ArrayBuilder,
    _meta: PhantomData<Meta>,
}

impl<Meta> EventTableBuilder<Meta> {
    pub(crate) fn new<M>(name: &'static str) -> Result<Self>
    where
        for<'de> Meta: Deserialize<'de>,
        for<'de> M: Deserialize<'de>,
    {
        let mut fields =
            Vec::<arrow_schema::FieldRef>::from_type::<Meta>(TracingOptions::default())?;
        fields.extend(Vec::<arrow_schema::FieldRef>::from_type::<M>(
            TracingOptions::default(),
        )?);

        Ok(Self {
            name,
            builder: ArrayBuilder::from_arrow(&fields)?,
            _meta: PhantomData,
        })
    }

    pub(crate) fn push<M>(&mut self, meta: Meta, message: M) -> Result<()>
    where
        Meta: Serialize,
        M: Serialize,
    {
        self.builder.push(EventTableRow::new(meta, message))?;
        Ok(())
    }

    pub(crate) fn flush_table(&mut self) -> Result<ArrowTable> {
        let batch = self
            .builder
            .to_record_batch()
            .with_context(|| format!("failed to flush {} table to Arrow", self.name))?;
        Ok(ArrowTable::new(self.name, vec![batch]))
    }

    pub(crate) fn into_table(self) -> Result<ArrowTable> {
        into_table(self.name, self.builder)
    }
}

pub(super) fn into_table(name: &'static str, builder: ArrayBuilder) -> Result<ArrowTable> {
    Ok(ArrowTable::new(
        name,
        vec![
            builder
                .into_record_batch()
                .with_context(|| format!("failed to convert {name} table to Arrow"))?,
        ],
    ))
}
