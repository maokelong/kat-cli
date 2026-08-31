use std::{fs::File, path::Path};

use arrow_array::{
    Array, BinaryArray, BooleanArray, PrimitiveArray, RecordBatch, StringArray, StructArray,
    types::ArrowPrimitiveType,
};
use arrow_schema::SchemaRef;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

pub(super) struct Relation {
    schema: SchemaRef,
    batches: Vec<RecordBatch>,
}

impl Relation {
    pub(super) fn open(root: &Path, name: &str) -> Self {
        let path = root.join(format!("{name}.parquet"));
        let file = File::open(&path)
            .unwrap_or_else(|error| panic!("failed to open relation {}: {error}", path.display()));
        let builder = ParquetRecordBatchReaderBuilder::try_new(file)
            .unwrap_or_else(|error| panic!("failed to read relation {}: {error}", path.display()));
        let schema = builder.schema().clone();
        let reader = builder.build().unwrap_or_else(|error| {
            panic!(
                "failed to build relation reader {}: {error}",
                path.display()
            )
        });
        let batches = reader
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_else(|error| panic!("failed to scan relation {}: {error}", path.display()));
        Self { schema, batches }
    }

    pub(super) fn schema(&self) -> &SchemaRef {
        &self.schema
    }

    pub(super) fn row_count(&self) -> usize {
        self.batches.iter().map(RecordBatch::num_rows).sum()
    }

    pub(super) fn primitive_values<T>(&self, name: &str) -> Vec<Option<T::Native>>
    where
        T: ArrowPrimitiveType,
        T::Native: Copy,
    {
        let mut values = Vec::new();
        for batch in &self.batches {
            let array = batch
                .column_by_name(name)
                .unwrap_or_else(|| panic!("relation has no column {name:?}"))
                .as_any()
                .downcast_ref::<PrimitiveArray<T>>()
                .unwrap_or_else(|| panic!("column {name:?} has an unexpected Arrow type"));
            values.extend(
                (0..array.len()).map(|index| (!array.is_null(index)).then(|| array.value(index))),
            );
        }
        values
    }

    pub(super) fn string_values(&self, name: &str) -> Vec<Option<String>> {
        let mut values = Vec::new();
        for batch in &self.batches {
            let array = batch
                .column_by_name(name)
                .unwrap_or_else(|| panic!("relation has no column {name:?}"))
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap_or_else(|| panic!("column {name:?} is not Utf8"));
            values.extend(
                (0..array.len())
                    .map(|index| (!array.is_null(index)).then(|| array.value(index).to_owned())),
            );
        }
        values
    }

    #[allow(dead_code)]
    pub(super) fn binary_values(&self, name: &str) -> Vec<Option<Vec<u8>>> {
        let mut values = Vec::new();
        for batch in &self.batches {
            let array = batch
                .column_by_name(name)
                .unwrap_or_else(|| panic!("relation has no column {name:?}"))
                .as_any()
                .downcast_ref::<BinaryArray>()
                .unwrap_or_else(|| panic!("column {name:?} is not Binary"));
            values.extend(
                (0..array.len())
                    .map(|index| (!array.is_null(index)).then(|| array.value(index).to_vec())),
            );
        }
        values
    }

    #[allow(dead_code)]
    pub(super) fn boolean_values(&self, name: &str) -> Vec<Option<bool>> {
        let mut values = Vec::new();
        for batch in &self.batches {
            let array = batch
                .column_by_name(name)
                .unwrap_or_else(|| panic!("relation has no column {name:?}"))
                .as_any()
                .downcast_ref::<BooleanArray>()
                .unwrap_or_else(|| panic!("column {name:?} is not Boolean"));
            values.extend(
                (0..array.len()).map(|index| (!array.is_null(index)).then(|| array.value(index))),
            );
        }
        values
    }

    pub(super) fn struct_nulls(&self, name: &str) -> Vec<bool> {
        let mut nulls = Vec::new();
        for batch in &self.batches {
            let array = batch
                .column_by_name(name)
                .unwrap_or_else(|| panic!("relation has no column {name:?}"))
                .as_any()
                .downcast_ref::<StructArray>()
                .unwrap_or_else(|| panic!("column {name:?} is not Struct"));
            nulls.extend((0..array.len()).map(|index| array.is_null(index)));
        }
        nulls
    }

    pub(super) fn struct_primitive_values<T>(
        &self,
        name: &str,
        child_name: &str,
    ) -> Vec<Option<T::Native>>
    where
        T: ArrowPrimitiveType,
        T::Native: Copy,
    {
        let mut values = Vec::new();
        for batch in &self.batches {
            let array = batch
                .column_by_name(name)
                .unwrap_or_else(|| panic!("relation has no column {name:?}"))
                .as_any()
                .downcast_ref::<StructArray>()
                .unwrap_or_else(|| panic!("column {name:?} is not Struct"));
            let child = array
                .column_by_name(child_name)
                .unwrap_or_else(|| panic!("Struct {name:?} has no child {child_name:?}"))
                .as_any()
                .downcast_ref::<PrimitiveArray<T>>()
                .unwrap_or_else(|| {
                    panic!("Struct child {name:?}.{child_name} has an unexpected Arrow type")
                });
            values.extend((0..array.len()).map(|index| {
                (!array.is_null(index) && !child.is_null(index)).then(|| child.value(index))
            }));
        }
        values
    }
}

pub(super) fn assert_no_staging(parent: &Path) {
    assert!(
        std::fs::read_dir(parent)
            .expect("destination parent can be listed")
            .all(|entry| !entry
                .expect("destination parent entry can be read")
                .file_name()
                .to_string_lossy()
                .starts_with(".kat-datasource-staging-")),
        "failed decode must not leave a staging directory"
    );
}
