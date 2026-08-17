use std::sync::Arc;

use anyhow::{Result, bail};
use arrow_schema::{DataType, Field, Schema};

use crate::{
    formats::hitrace::profiler::{PluginEnvelope, PluginEnvelopeKind, decode_payload},
    generated_native_hook_source_emitter::{
        append_batch_native_hook_data_root, append_native_hook_config_root,
        profiler_clock_id_symbols, protobuf_source_specs,
    },
    proto::{BatchNativeHookData, NativeHookConfig},
    protobuf_source::{
        EnumOriginSpec, EstimatedRow, PreparedSourceTables, RelationSlot, RelationSpec,
        SourceTableCapture, SpoolOptions,
    },
};

const PROFILER_PAYLOAD_OCCURRENCE: &str = "profiler_payload_occurrence";

enum NativeHookRoot {
    BatchData,
    Config,
}

/// Native Hook roots 与 profiler envelope provenance 的 dormant capture。
pub(crate) struct NativeHookSourceCapture {
    capture: SourceTableCapture,
    occurrence: RelationSlot,
    terminal_error: Option<String>,
    clock_admission: NativeHookClockAdmission,
}

impl NativeHookSourceCapture {
    pub(crate) fn new(options: SpoolOptions) -> Result<Self> {
        let (mut relations, mut enum_origins) = protobuf_source_specs();
        let occurrence = RelationSlot::new(relations.len());
        relations.push(profiler_payload_occurrence_spec());
        let (clock_enum_fqn, clock_symbols) = profiler_clock_id_symbols();
        enum_origins.push(EnumOriginSpec::new(
            occurrence,
            "clock_id",
            clock_enum_fqn,
            clock_symbols,
        ));
        Ok(Self {
            capture: SourceTableCapture::new(relations, enum_origins, options)?,
            occurrence,
            terminal_error: None,
            clock_admission: NativeHookClockAdmission::default(),
        })
    }

    /// 只认领四条固定 Native Hook route；未绑定 payload 不会被解码。
    pub(crate) fn try_claim(&mut self, envelope: &PluginEnvelope<'_>) -> Result<bool> {
        self.ensure_healthy()?;
        let Some(root) = native_hook_root(envelope) else {
            return Ok(false);
        };

        let result = self.claim(root, envelope);
        if let Err(error) = result {
            self.terminal_error = Some(format!("{error:#}"));
            return Err(error);
        }
        Ok(true)
    }

    pub(crate) fn finish(self) -> Result<PreparedSourceTables> {
        self.ensure_healthy()?;
        self.clock_admission.validate()?;
        self.capture.finish()
    }

    fn claim(&mut self, root: NativeHookRoot, envelope: &PluginEnvelope<'_>) -> Result<()> {
        match root {
            NativeHookRoot::BatchData => {
                let value: BatchNativeHookData = decode_payload(envelope)?;
                let occurrence_row_id = self.append_occurrence(envelope)?;
                append_batch_native_hook_data_root(&mut self.capture, occurrence_row_id, &value)?;
                self.clock_admission
                    .observe_batch(&value, envelope.clock_id);
            }
            NativeHookRoot::Config => {
                let value: NativeHookConfig = decode_payload(envelope)?;
                let occurrence_row_id = self.append_occurrence(envelope)?;
                append_native_hook_config_root(&mut self.capture, occurrence_row_id, &value)?;
                self.clock_admission.observe_config(&value);
            }
        }
        Ok(())
    }

    fn append_occurrence(&mut self, envelope: &PluginEnvelope<'_>) -> Result<u64> {
        let row_id = self.capture.allocate_row_id(self.occurrence)?;
        let row = ProfilerPayloadOccurrenceRow {
            row_id,
            envelope_name: envelope.envelope_name,
            status: envelope.status,
            clock_id: envelope.clock_id,
            tv_sec: envelope.tv_sec,
            tv_nsec: envelope.tv_nsec,
            version: envelope.version,
            sample_interval: envelope.sample_interval,
        };
        self.capture.append_row(self.occurrence, &row)?;
        Ok(row_id)
    }

    fn ensure_healthy(&self) -> Result<()> {
        if let Some(source) = &self.terminal_error {
            bail!("Native Hook Source capture is poisoned by an earlier failure: {source}");
        }
        Ok(())
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
        use crate::protobuf_source::EstimatedValue;

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
            crate::protobuf_source::add_estimated_bytes(&mut total, bytes)?;
        }
        Ok(total)
    }
}

#[derive(Default)]
struct NativeHookClockAdmission {
    requires_config: bool,
    config_clock: Option<NativeHookClock>,
    unsupported_config_clock: Option<String>,
    different_config_clock: Option<NativeHookClock>,
    first_event_envelope_clock: Option<i32>,
    different_event_envelope_clock: Option<i32>,
}

impl NativeHookClockAdmission {
    fn observe_batch(&mut self, batch: &BatchNativeHookData, envelope_clock: i32) {
        if batch.events.is_empty() {
            return;
        }
        self.requires_config = true;
        remember_first_difference(
            &mut self.first_event_envelope_clock,
            &mut self.different_event_envelope_clock,
            envelope_clock,
        );
    }

    fn observe_config(&mut self, config: &NativeHookConfig) {
        if let Some(clock) = NativeHookClock::parse(&config.clock) {
            remember_first_difference(
                &mut self.config_clock,
                &mut self.different_config_clock,
                clock,
            );
        } else if self.unsupported_config_clock.is_none() {
            self.unsupported_config_clock = Some(config.clock.clone());
        }
    }

    fn validate(&self) -> Result<()> {
        if !self.requires_config {
            return Ok(());
        }
        if let Some(clock) = &self.unsupported_config_clock {
            bail!("unsupported Native Hook config clock {clock:?}");
        }
        if let (Some(first), Some(second)) = (self.config_clock, self.different_config_clock) {
            bail!(
                "conflicting Native Hook config clocks {:?} and {:?}",
                first.config_name,
                second.config_name
            );
        }
        let Some(config_clock) = self.config_clock else {
            bail!("Native Hook events require a Native Hook config clock");
        };
        if let Some(envelope_clock) = self.mismatched_envelope_clock(config_clock) {
            bail!(
                "Native Hook config clock {:?} expects profiler envelope clock_id {}, but observed {envelope_clock}",
                config_clock.config_name,
                config_clock.envelope_clock_id
            );
        }
        Ok(())
    }

    fn mismatched_envelope_clock(&self, config_clock: NativeHookClock) -> Option<i32> {
        let expected = config_clock.envelope_clock_id;
        self.first_event_envelope_clock
            .filter(|observed| *observed != expected)
            .or_else(|| {
                self.different_event_envelope_clock
                    .filter(|observed| *observed != expected)
            })
    }
}

fn remember_first_difference<T: Copy + Eq>(
    first: &mut Option<T>,
    different: &mut Option<T>,
    observed: T,
) {
    match *first {
        None => *first = Some(observed),
        Some(first) if first != observed && different.is_none() => *different = Some(observed),
        Some(_) => {}
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct NativeHookClock {
    config_name: &'static str,
    envelope_clock_id: i32,
}

impl NativeHookClock {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "" | "realtime" => Some(Self::new("realtime", 0)),
            "mono" => Some(Self::new("mono", 1)),
            "mono_raw" => Some(Self::new("mono_raw", 4)),
            "boot" => Some(Self::new("boot", 7)),
            _ => None,
        }
    }

    const fn new(config_name: &'static str, envelope_clock_id: i32) -> Self {
        Self {
            config_name,
            envelope_clock_id,
        }
    }
}

fn native_hook_root(envelope: &PluginEnvelope<'_>) -> Option<NativeHookRoot> {
    match (envelope.envelope_name, envelope.kind) {
        ("nativehook", PluginEnvelopeKind::Data) | ("hookdaemon", PluginEnvelopeKind::Data) => {
            Some(NativeHookRoot::BatchData)
        }
        ("nativehook_config", PluginEnvelopeKind::Config)
        | ("hookdaemon_config", PluginEnvelopeKind::Config) => Some(NativeHookRoot::Config),
        _ => None,
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
