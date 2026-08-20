//! Profiler envelope provenance 的私有 capture adapter。
//!
//! `profiler_payload_occurrence` 是 transport envelope 的受控投影，不是 descriptor-derived
//! protobuf root。本模块独占其 Schema、row、逻辑字节估算、enum origin 与 relation slot。

use std::sync::Arc;

use anyhow::Result;
use arrow_schema::{DataType, Field, Schema};

use crate::{
    dataset_writer::DatasetTableFactory, formats::hitrace::profiler::PluginEnvelope,
    generated_profiler_source_emitter::profiler_clock_id_symbols,
};

use super::{
    EnumOriginSpec, EstimatedRow, PreparedSourceTables, RelationSpec, SourceTableCapture,
    SourceTableLayout, SpoolOptions,
};

const PROFILER_PAYLOAD_OCCURRENCE: &str = "profiler_payload_occurrence";

/// Descriptor compiler 交给 profiler adapter 的 opaque payload layout。
pub(crate) struct ProfilerPayloadLayout(SourceTableLayout);

impl ProfilerPayloadLayout {
    pub(crate) fn from_generated(
        relations: Vec<RelationSpec>,
        enum_origins: Vec<EnumOriginSpec>,
    ) -> Self {
        Self(SourceTableLayout::from_generated(relations, enum_origins))
    }
}

pub(crate) struct ProfilerPayloadCapture {
    capture: SourceTableCapture,
    occurrence: super::RelationSlot,
}

impl ProfilerPayloadCapture {
    #[allow(dead_code)]
    pub(crate) fn new(layout: ProfilerPayloadLayout, options: SpoolOptions) -> Result<Self> {
        Self::build(layout, options, None)
    }

    pub(crate) fn new_staged(
        layout: ProfilerPayloadLayout,
        options: SpoolOptions,
        tables: DatasetTableFactory,
    ) -> Result<Self> {
        Self::build(layout, options, Some(tables))
    }

    fn build(
        layout: ProfilerPayloadLayout,
        options: SpoolOptions,
        tables: Option<DatasetTableFactory>,
    ) -> Result<Self> {
        let mut layout = layout.0;
        let occurrence = layout.append_relation(profiler_payload_occurrence_spec());
        let (clock_enum_fqn, clock_symbols) = profiler_clock_id_symbols();
        layout.append_enum_origin(EnumOriginSpec::new(
            occurrence,
            "clock_id",
            clock_enum_fqn,
            clock_symbols,
        ));
        Ok(Self {
            capture: match tables {
                Some(tables) => layout.into_staged_capture(options, tables)?,
                None => layout.into_capture(options)?,
            },
            occurrence,
        })
    }

    pub(crate) fn append_bound_payload<T>(
        &mut self,
        envelope: &PluginEnvelope<'_>,
        value: &T,
        emit_root: fn(&mut SourceTableCapture, u64, &T) -> Result<()>,
    ) -> Result<()> {
        let row_id = self.capture.allocate_row_id(self.occurrence)?;
        self.capture.append_row(
            self.occurrence,
            &ProfilerPayloadOccurrenceRow {
                row_id,
                envelope_name: envelope.envelope_name,
                status: envelope.status,
                clock_id: envelope.clock_id,
                tv_sec: envelope.tv_sec,
                tv_nsec: envelope.tv_nsec,
                version: envelope.version,
                sample_interval: envelope.sample_interval,
            },
        )?;
        emit_root(&mut self.capture, row_id, value)
    }

    pub(crate) fn finish(self) -> Result<PreparedSourceTables> {
        self.capture.finish()
    }
}

#[derive(serde::Serialize)]
struct ProfilerPayloadOccurrenceRow<'a> {
    #[serde(rename = "_kat_row_id")]
    row_id: u64,
    envelope_name: &'a str,
    status: u32,
    clock_id: i32,
    tv_sec: u64,
    tv_nsec: u64,
    version: &'a str,
    sample_interval: u32,
}

impl EstimatedRow for ProfilerPayloadOccurrenceRow<'_> {
    fn estimated_bytes(&self) -> Result<usize> {
        use super::EstimatedValue;

        let mut total = 0;
        for bytes in [
            self.row_id.estimated_bytes()?,
            self.envelope_name.estimated_bytes()?,
            self.status.estimated_bytes()?,
            self.clock_id.estimated_bytes()?,
            self.tv_sec.estimated_bytes()?,
            self.tv_nsec.estimated_bytes()?,
            self.version.estimated_bytes()?,
            self.sample_interval.estimated_bytes()?,
        ] {
            super::add_estimated_bytes(&mut total, bytes)?;
        }
        Ok(total)
    }
}

fn profiler_payload_occurrence_spec() -> RelationSpec {
    RelationSpec::new(
        PROFILER_PAYLOAD_OCCURRENCE,
        Arc::new(Schema::new(vec![
            Field::new("_kat_row_id", DataType::UInt64, false),
            Field::new("envelope_name", DataType::Utf8, false),
            Field::new("status", DataType::UInt32, false),
            Field::new("clock_id", DataType::Int32, false),
            Field::new("tv_sec", DataType::UInt64, false),
            Field::new("tv_nsec", DataType::UInt64, false),
            Field::new("version", DataType::Utf8, false),
            Field::new("sample_interval", DataType::UInt32, false),
        ])),
    )
}
