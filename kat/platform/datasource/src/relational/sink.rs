use std::collections::{BTreeMap, HashMap};

use anyhow::Result;

use crate::{
    dataset_writer::DatasetWriter,
    record::{DecodedPayload, TraceRecord, TraceRecordSink},
};

use super::{
    plan_exec::CompiledRootPlan,
    table_batch::{ParquetWriteWorker, TableBuffer, flush_table},
    table_data::Ordinals,
};

pub(crate) struct RelationalDatasetSink {
    pub(super) table_writer: ParquetWriteWorker,
    pub(super) source_index: u64,
    pub(super) compiled_plans: HashMap<String, CompiledRootPlan>,
    pub(super) tables: BTreeMap<String, TableBuffer>,
    pub(super) parent_indexes: HashMap<String, HashMap<Ordinals, u64>>,
}

impl RelationalDatasetSink {
    pub(crate) fn new(dataset_writer: DatasetWriter) -> Result<Self> {
        Ok(Self {
            table_writer: ParquetWriteWorker::new(dataset_writer)?,
            source_index: 0,
            compiled_plans: HashMap::new(),
            tables: BTreeMap::new(),
            parent_indexes: HashMap::new(),
        })
    }

    pub(crate) fn finish(mut self) -> Result<DatasetWriter> {
        self.flush_all_tables()?;
        self.table_writer.finish()
    }

    pub(crate) fn push_payload(&mut self, payload: DecodedPayload) -> Result<()> {
        self.emit_payload(self.source_index, &payload)?;
        self.parent_indexes.clear();
        Ok(())
    }

    fn flush_all_tables(&mut self) -> Result<()> {
        let table_names = self.tables.keys().cloned().collect::<Vec<_>>();
        for table_name in table_names {
            self.flush_table(&table_name)?;
        }

        Ok(())
    }

    fn flush_table(&mut self, table_name: &str) -> Result<()> {
        flush_table(&self.table_writer, &mut self.tables, table_name)
    }
}

impl TraceRecordSink for RelationalDatasetSink {
    fn accepts_decoded_payloads(&self) -> bool {
        true
    }

    fn accepts_source_records(&self) -> bool {
        false
    }

    fn push(&mut self, record: TraceRecord) -> Result<()> {
        if let TraceRecord::DecodedPayload(payload) = record {
            self.push_payload(*payload)?;
        }

        Ok(())
    }
}
