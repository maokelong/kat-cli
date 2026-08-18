use std::{
    fs::File,
    io::{Seek, SeekFrom},
};

use anyhow::{Context, Result, bail};
use parquet::arrow::{
    ArrowWriter,
    arrow_reader::{
        ArrowReaderMetadata, ArrowReaderOptions, ParquetRecordBatchReader,
        ParquetRecordBatchReaderBuilder,
    },
};

use crate::dataset_writer::DatasetWriter;

use super::{EstimatedRow, RelationSpec, SpoolOptions};

pub(super) struct ActiveTable {
    spec: RelationSpec,
    writer: ArrowWriter<File>,
    builder: serde_arrow::ArrayBuilder,
    buffered_rows: usize,
    buffered_bytes: usize,
    total_rows: u64,
    options: SpoolOptions,
}

impl ActiveTable {
    pub(super) fn new(spec: RelationSpec, options: SpoolOptions) -> Result<Self> {
        let file = tempfile::tempfile().with_context(|| {
            format!(
                "failed to create bounded protobuf Source spool for table {:?}",
                spec.name
            )
        })?;
        let writer = ArrowWriter::try_new(file, spec.schema.clone(), None).with_context(|| {
            format!(
                "failed to open protobuf Source Parquet spool for table {:?}",
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
            total_rows: 0,
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
        self.total_rows = self
            .total_rows
            .checked_add(1)
            .context("protobuf Source table row count overflows")?;
        self.buffered_bytes = self
            .buffered_bytes
            .checked_add(row_estimated_bytes)
            .context("protobuf Source buffered byte estimate overflows")?;
        if spool_limit_reached(self.buffered_rows, self.buffered_bytes, self.options) {
            self.flush()?;
        }
        Ok(())
    }

    pub(super) fn has_rows(&self) -> bool {
        self.total_rows != 0
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
                "failed to write protobuf Source Parquet spool for table {:?}",
                self.spec.name
            )
        })?;
        self.writer.flush().with_context(|| {
            format!(
                "failed to finish protobuf Source Parquet row group for table {:?}",
                self.spec.name
            )
        })?;
        self.buffered_rows = 0;
        self.buffered_bytes = 0;
        Ok(())
    }

    pub(super) fn prepare(mut self) -> Result<PreparedSourceTable> {
        if !self.has_rows() {
            bail!(
                "cannot prepare empty protobuf Source table {:?}",
                self.spec.name
            );
        }
        self.flush()?;
        let mut file = self.writer.into_inner().with_context(|| {
            format!(
                "failed to finish protobuf Source Parquet spool for table {:?}",
                self.spec.name
            )
        })?;
        file.seek(SeekFrom::Start(0)).with_context(|| {
            format!(
                "failed to rewind protobuf Source Parquet spool for table {:?}",
                self.spec.name
            )
        })?;

        let metadata = ArrowReaderMetadata::load(&file, ArrowReaderOptions::default())
            .with_context(|| {
                format!(
                    "failed to read protobuf Source Parquet metadata for table {:?}",
                    self.spec.name
                )
            })?;
        if metadata.schema().as_ref() != self.spec.schema.as_ref() {
            bail!(
                "protobuf Source Parquet schema differs from planned schema for table {:?}: planned={:?} actual={:?}",
                self.spec.name,
                self.spec.schema,
                metadata.schema()
            );
        }
        let expected_rows = i64::try_from(self.total_rows).with_context(|| {
            format!(
                "protobuf Source table {:?} row count exceeds Parquet Int64 metadata",
                self.spec.name
            )
        })?;
        let actual_rows = metadata.metadata().file_metadata().num_rows();
        if actual_rows != expected_rows {
            bail!(
                "protobuf Source Parquet row count differs for table {:?}: expected {expected_rows}, actual {actual_rows}",
                self.spec.name
            );
        }
        for (index, row_group) in metadata.metadata().row_groups().iter().enumerate() {
            if row_group.num_rows() > self.options.max_buffered_rows as i64 {
                bail!(
                    "protobuf Source table {:?} row group {index} has {} rows, above configured limit {}",
                    self.spec.name,
                    row_group.num_rows(),
                    self.options.max_buffered_rows
                );
            }
            let expected_group_rows = usize::try_from(row_group.num_rows()).with_context(|| {
                format!(
                    "protobuf Source table {:?} row group {index} has an invalid row count {}",
                    self.spec.name,
                    row_group.num_rows()
                )
            })?;
            preflight_row_group(
                &file,
                &metadata,
                self.options.max_buffered_rows,
                &self.spec,
                index,
                expected_group_rows,
            )?;
        }

        let row_group_count = metadata.metadata().num_row_groups();
        file.seek(SeekFrom::Start(0)).with_context(|| {
            format!(
                "failed to rewind preflighted protobuf Source spool for table {:?}",
                self.spec.name
            )
        })?;
        let reader = ParquetRecordBatchReaderBuilder::new_with_metadata(file, metadata)
            .with_batch_size(self.options.max_buffered_rows)
            .build()
            .with_context(|| {
                format!(
                    "failed to prepare final protobuf Source reader for table {:?}",
                    self.spec.name
                )
            })?;
        Ok(PreparedSourceTable {
            spec: self.spec,
            reader,
            preflighted_row_group_count: row_group_count,
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

pub(super) struct PreparedSourceTable {
    spec: RelationSpec,
    reader: ParquetRecordBatchReader,
    preflighted_row_group_count: usize,
}

fn open_row_group_reader(
    file: &File,
    metadata: &ArrowReaderMetadata,
    batch_size: usize,
    table_name: &str,
    row_group: usize,
    phase: &str,
) -> Result<ParquetRecordBatchReader> {
    let mut reader_file = file.try_clone().with_context(|| {
        format!(
            "{phase}: failed to clone protobuf Source table {table_name:?} row group {row_group}"
        )
    })?;
    reader_file.seek(SeekFrom::Start(0)).with_context(|| {
        format!(
            "{phase}: failed to rewind protobuf Source table {table_name:?} row group {row_group}"
        )
    })?;
    ParquetRecordBatchReaderBuilder::new_with_metadata(reader_file, metadata.clone())
        .with_row_groups(vec![row_group])
        .with_batch_size(batch_size)
        .build()
        .with_context(|| {
            format!(
                "{phase}: failed to open protobuf Source table {table_name:?} row group {row_group}"
            )
        })
}

fn preflight_row_group(
    file: &File,
    metadata: &ArrowReaderMetadata,
    batch_size: usize,
    spec: &RelationSpec,
    row_group: usize,
    expected_rows: usize,
) -> Result<()> {
    let mut reader = open_row_group_reader(
        file,
        metadata,
        batch_size,
        spec.name,
        row_group,
        "protobuf Source preflight",
    )?;
    let mut batch_count = 0_usize;
    let mut actual_rows = 0_usize;
    for batch in &mut reader {
        let batch = batch.with_context(|| {
            format!(
                "protobuf Source preflight failed to read table {:?} row group {row_group}",
                spec.name
            )
        })?;
        batch_count = batch_count
            .checked_add(1)
            .context("protobuf Source preflight batch count overflows")?;
        if batch_count > 1 {
            bail!(
                "protobuf Source preflight table {:?} row group {row_group} produced more than one batch",
                spec.name
            );
        }
        if batch.schema().as_ref() != spec.schema.as_ref() {
            bail!(
                "protobuf Source preflight batch schema differs for table {:?} row group {row_group}: planned={:?} actual={:?}",
                spec.name,
                spec.schema,
                batch.schema()
            );
        }
        actual_rows = actual_rows
            .checked_add(batch.num_rows())
            .context("protobuf Source preflight row count overflows")?;
    }
    if batch_count != 1 {
        bail!(
            "protobuf Source preflight table {:?} row group {row_group} produced no batch",
            spec.name
        );
    }
    if actual_rows != expected_rows {
        bail!(
            "protobuf Source preflight row count differs for table {:?} row group {row_group}: expected {expected_rows}, actual {actual_rows}",
            spec.name
        );
    }
    Ok(())
}

pub(crate) struct PreparedSourceTables {
    tables: Vec<PreparedSourceTable>,
}

impl PreparedSourceTables {
    pub(super) fn new(tables: Vec<PreparedSourceTable>) -> Self {
        Self { tables }
    }

    // 该只读观察点用于验证 preflight 边界；production drain 不应为消费计数而增加分支。
    #[allow(dead_code)]
    pub(crate) fn preflighted_row_group_count(&self, table: &str) -> Option<usize> {
        self.tables
            .iter()
            .find(|prepared| prepared.spec.name == table)
            .map(|prepared| prepared.preflighted_row_group_count)
    }

    pub(crate) fn write_into(mut self, writer: &mut DatasetWriter) -> Result<()> {
        for prepared in &mut self.tables {
            let mut table = writer
                .begin_table(prepared.spec.name, prepared.spec.schema.clone())
                .with_context(|| {
                    format!(
                        "failed to begin protobuf Source Dataset table {:?}",
                        prepared.spec.name
                    )
                })?;
            for batch in &mut prepared.reader {
                let batch = batch.with_context(|| {
                    format!(
                        "new I/O/resource failure after Dataset begin while reading preflighted protobuf Source table {:?}",
                        prepared.spec.name
                    )
                })?;
                table.write(&batch).with_context(|| {
                    format!(
                        "failed to write protobuf Source Dataset table {:?}",
                        prepared.spec.name
                    )
                })?;
            }
            table.finish().with_context(|| {
                format!(
                    "failed to finish protobuf Source Dataset table {:?}",
                    prepared.spec.name
                )
            })?;
        }
        Ok(())
    }
}
