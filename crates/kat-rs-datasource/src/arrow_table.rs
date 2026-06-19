// In-memory Arrow tables consumed by query registration and dataset writers.

use arrow_array::RecordBatch;

pub(crate) struct ArrowTable {
    pub(crate) name: &'static str,
    pub(crate) batches: Vec<RecordBatch>,
}

impl ArrowTable {
    pub(crate) fn new(name: &'static str, batches: Vec<RecordBatch>) -> Self {
        Self { name, batches }
    }
}

pub(crate) struct ArrowTableSet {
    pub(crate) tables: Vec<ArrowTable>,
}

impl ArrowTableSet {
    pub(crate) fn new(tables: Vec<ArrowTable>) -> Self {
        Self { tables }
    }
}
