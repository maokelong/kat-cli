//! Converts Arrow query results into the JSON array returned by datasource APIs.

use anyhow::Result;
use arrow_array::RecordBatch;
use arrow_json::writer::{JsonArray, WriterBuilder};
use serde_json::Value;

pub(crate) fn batches_to_json(batches: &[RecordBatch]) -> Result<Value> {
    let batch_refs = batches.iter().collect::<Vec<_>>();
    let mut buffer = Vec::new();
    let mut writer = WriterBuilder::new()
        .with_explicit_nulls(true)
        .build::<_, JsonArray>(&mut buffer);

    writer.write_batches(&batch_refs)?;
    writer.finish()?;
    drop(writer);

    Ok(serde_json::from_slice(&buffer)?)
}
