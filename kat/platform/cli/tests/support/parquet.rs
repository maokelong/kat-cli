use std::{fs, path::Path, sync::Arc};

use arrow_array::{Int64Array, RecordBatch};
use arrow_schema::{DataType, Field, Schema};
use parquet::arrow::ArrowWriter;

pub fn write_i64(path: &Path, column: &str, values: &[i64]) {
    let schema = Arc::new(Schema::new(vec![Field::new(
        column,
        DataType::Int64,
        false,
    )]));
    let batch = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![Arc::new(Int64Array::from(values.to_vec()))],
    )
    .expect("build test record batch");
    let mut writer = ArrowWriter::try_new(
        fs::File::create(path).expect("create test Parquet file"),
        schema,
        None,
    )
    .expect("create test Parquet writer");
    writer.write(&batch).expect("write test Parquet batch");
    writer.close().expect("finish test Parquet file");
}
