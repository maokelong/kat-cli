use std::{marker::PhantomData, sync::Arc};

use anyhow::{Context, Result};
use arrow_schema::{DataType, Field, FieldRef, Schema};
use serde::{Deserialize, Serialize};
use serde_arrow::{
    ArrayBuilder,
    schema::{SchemaLike, TracingOptions},
};

use crate::relation_writer::{RelationFileWriter, RelationWriter};

const BATCH_ROWS: usize = 8_192;

pub(crate) struct TableWriter<T> {
    name: &'static str,
    builder: ArrayBuilder,
    writer: RelationFileWriter,
    buffered_rows: usize,
    _row: PhantomData<T>,
}

impl<T> TableWriter<T>
where
    for<'de> T: Deserialize<'de>,
    T: Serialize,
{
    pub(crate) fn new(relations: &RelationWriter, name: &'static str) -> Result<Self> {
        let fields = Vec::<FieldRef>::from_type::<T>(TracingOptions::default())?
            .into_iter()
            .map(|field| {
                if field.data_type() == &DataType::LargeUtf8 {
                    Arc::new(Field::new(
                        field.name(),
                        DataType::Utf8,
                        field.is_nullable(),
                    ))
                } else {
                    field
                }
            })
            .collect::<Vec<_>>();
        let schema = Arc::new(Schema::new(fields.clone()));
        Ok(Self {
            name,
            builder: ArrayBuilder::from_arrow(&fields)?,
            writer: relations.begin(name, schema)?,
            buffered_rows: 0,
            _row: PhantomData,
        })
    }

    pub(crate) fn push(&mut self, row: T) -> Result<()> {
        self.builder.push(row)?;
        self.buffered_rows += 1;
        if self.buffered_rows == BATCH_ROWS {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        if self.buffered_rows == 0 {
            return Ok(());
        }
        let batch = self
            .builder
            .to_record_batch()
            .with_context(|| format!("failed to build {:?} batch", self.name))?;
        self.writer.write(&batch)?;
        self.buffered_rows = 0;
        Ok(())
    }

    pub(crate) fn finish(mut self) -> Result<()> {
        self.flush()?;
        self.writer.finish()?;
        Ok(())
    }
}
