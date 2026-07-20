use std::collections::{BTreeMap, HashMap};

use anyhow::Result;

use crate::{
    dataset::DatasetWriter,
    record::{DecodedPayload, TraceRecord, TraceRecordSink},
};

use super::{
    plan_exec::CompiledRootPlan,
    table_batch::{ParquetWriteWorker, TableBuffer, flush_table},
    table_data::Ordinals,
};

const RELATIONAL_PAYLOAD_CHUNK_MAX_RECORDS: usize = 8;

pub(crate) struct RelationalDatasetSink {
    pub(super) table_writer: ParquetWriteWorker,
    pub(super) source_index: u64,
    pub(super) compiled_plans: HashMap<String, CompiledRootPlan>,
    pub(super) tables: BTreeMap<String, TableBuffer>,
    pub(super) parent_indexes: HashMap<String, HashMap<Ordinals, u64>>,
    pending_payloads: PayloadChunk,
}

struct PayloadChunk {
    payloads: Vec<PendingPayload>,
}

struct PendingPayload {
    payload: DecodedPayload,
}

impl PayloadChunk {
    fn empty() -> Self {
        Self {
            payloads: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.payloads.is_empty()
    }

    fn push(&mut self, payload: DecodedPayload) {
        self.payloads.push(PendingPayload { payload });
    }

    fn should_flush(&self) -> bool {
        self.payloads.len() >= RELATIONAL_PAYLOAD_CHUNK_MAX_RECORDS
    }
}

impl RelationalDatasetSink {
    pub(crate) fn new(dataset_writer: DatasetWriter) -> Result<Self> {
        Ok(Self {
            table_writer: ParquetWriteWorker::new(dataset_writer)?,
            source_index: 0,
            compiled_plans: HashMap::new(),
            tables: BTreeMap::new(),
            parent_indexes: HashMap::new(),
            pending_payloads: PayloadChunk::empty(),
        })
    }

    pub(crate) fn finish(mut self) -> Result<DatasetWriter> {
        self.flush_pending_payloads()?;
        self.flush_all_tables()?;
        self.table_writer.finish()
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

    fn flush_pending_payloads(&mut self) -> Result<()> {
        if self.pending_payloads.is_empty() {
            return Ok(());
        }

        let chunk = std::mem::replace(&mut self.pending_payloads, PayloadChunk::empty());
        self.execute_payload_chunk(chunk)
    }

    fn execute_payload_chunk(&mut self, chunk: PayloadChunk) -> Result<()> {
        for pending_payload in chunk.payloads {
            let payload = pending_payload.payload;
            let _ = &payload.plugin_name;
            self.emit_payload(self.source_index, &payload)?;
            self.parent_indexes.clear();
        }

        Ok(())
    }
}

impl TraceRecordSink for RelationalDatasetSink {
    fn push(&mut self, record: TraceRecord) -> Result<()> {
        let TraceRecord::DecodedPayload(payload) = record;
        self.pending_payloads.push(*payload);
        if self.pending_payloads.should_flush() {
            self.flush_pending_payloads()?;
        }

        Ok(())
    }
}
