//! Defines the stable macro and traits for saving typed protobuf rows as Arrow batches.

use std::sync::Arc;

use anyhow::Result;
use arrow_array::RecordBatch;
use arrow_schema::Schema;

pub(crate) trait ArrowRow: Sized {
    type Writer: ArrowRowWriter<Self>;

    fn arrow_schema() -> Arc<Schema>;

    fn new_arrow_writer(capacity: usize) -> Self::Writer;

    fn append_to_arrow(&self, writer: &mut Self::Writer) -> Result<()> {
        writer.append(self)
    }
}

pub(crate) trait ArrowRowWriter<T> {
    fn append(&mut self, row: &T) -> Result<()>;

    fn finish(self) -> Result<RecordBatch>;
}

pub(crate) fn save_rows_to_arrow_batch<T>(rows: impl IntoIterator<Item = T>) -> Result<RecordBatch>
where
    T: ArrowRow,
{
    let rows = rows.into_iter();
    let capacity = rows.size_hint().0;
    let mut writer = T::new_arrow_writer(capacity);

    for row in rows {
        row.append_to_arrow(&mut writer)?;
    }

    writer.finish()
}

macro_rules! save_to_arrow {
    ($rows:expr) => {{ $crate::arrow::save_rows_to_arrow_batch($rows) }};
}

pub(crate) use save_to_arrow;
