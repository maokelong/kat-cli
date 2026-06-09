//! Defines the trait contract generated protobuf rows use to build Arrow batches.

use std::sync::Arc;

use anyhow::Result;
use arrow_array::RecordBatch;
use arrow_schema::Schema;

pub(crate) trait ArrowRow: Sized {
    type Writer: ArrowRowWriter<Self>;

    fn arrow_schema() -> Arc<Schema>;

    fn new_arrow_writer(capacity: usize) -> Self::Writer;

    fn record_batch_from(rows: impl IntoIterator<Item = Self>) -> Result<RecordBatch> {
        let rows = rows.into_iter();
        let capacity = rows.size_hint().0;
        let mut writer = Self::new_arrow_writer(capacity);

        for row in rows {
            writer.append(&row);
        }

        writer.finish()
    }
}

pub(crate) trait ArrowRowWriter<T> {
    fn append(&mut self, row: &T);

    fn finish(self) -> Result<RecordBatch>;
}
