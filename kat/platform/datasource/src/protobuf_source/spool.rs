use anyhow::{Context, Result};

use crate::dataset_writer::{DatasetTableFactory, DatasetTableWriter};

use super::{EstimatedRow, RelationSpec, SpoolOptions};

pub(super) struct ActiveTable {
    spec: RelationSpec,
    writer: DatasetTableWriter,
    builder: serde_arrow::ArrayBuilder,
    buffered_rows: usize,
    buffered_bytes: usize,
    options: SpoolOptions,
}

impl ActiveTable {
    pub(super) fn new(
        spec: RelationSpec,
        options: SpoolOptions,
        tables: &DatasetTableFactory,
    ) -> Result<Self> {
        let writer = tables
            .begin_table(spec.name, spec.schema.clone())
            .with_context(|| {
                format!(
                    "failed to open staged protobuf Source table {:?}",
                    spec.name
                )
            })?;
        let builder = serde_arrow::ArrayBuilder::from_arrow(spec.schema.fields())
            .context("failed to create protobuf Source Arrow row serializer")?;
        Ok(Self {
            spec,
            writer,
            builder,
            buffered_rows: 0,
            buffered_bytes: 0,
            options,
        })
    }

    pub(super) fn append_row<T>(&mut self, row: &T) -> Result<()>
    where
        T: EstimatedRow,
    {
        let row_estimated_bytes = row.estimated_bytes()?;
        self.builder.push(row).with_context(|| {
            format!(
                "failed to serialize protobuf Source row for table {:?}",
                self.spec.name
            )
        })?;

        self.buffered_rows = self
            .buffered_rows
            .checked_add(1)
            .context("protobuf Source buffered row count overflows")?;
        self.buffered_bytes = self
            .buffered_bytes
            .checked_add(row_estimated_bytes)
            .context("protobuf Source buffered byte estimate overflows")?;
        if spool_limit_reached(self.buffered_rows, self.buffered_bytes, self.options) {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        if self.buffered_rows == 0 {
            return Ok(());
        }
        let batch = self.builder.to_record_batch().with_context(|| {
            format!(
                "failed to build protobuf Source batch for table {:?}",
                self.spec.name
            )
        })?;
        self.writer.write(&batch).with_context(|| {
            format!(
                "failed to write staged protobuf Source table {:?}",
                self.spec.name
            )
        })?;
        self.buffered_rows = 0;
        self.buffered_bytes = 0;
        Ok(())
    }

    pub(super) fn finish(mut self) -> Result<()> {
        self.flush()?;
        self.writer.finish().with_context(|| {
            format!(
                "failed to finish staged protobuf Source table {:?}",
                self.spec.name
            )
        })
    }
}

pub(super) fn spool_limit_reached(
    buffered_rows: usize,
    buffered_bytes: usize,
    options: SpoolOptions,
) -> bool {
    buffered_rows >= options.max_buffered_rows || buffered_bytes >= options.max_buffered_bytes
}
