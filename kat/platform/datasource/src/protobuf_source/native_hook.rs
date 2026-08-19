use anyhow::{Result, bail};

use crate::{
    formats::hitrace::profiler::{PluginEnvelope, PluginEnvelopeKind, decode_payload},
    generated_profiler_source_emitter::{
        append_batch_native_hook_data_root, append_native_hook_config_root, protobuf_source_layout,
    },
    proto::{BatchNativeHookData, NativeHookConfig, kat::hitrace::profiler_plugin_data::ClockId},
    protobuf_source::{
        PreparedSourceTables, SpoolOptions, profiler_occurrence::ProfilerPayloadCapture,
    },
};

enum NativeHookRoot {
    BatchData,
    Config,
}

/// Native Hook roots 与 profiler envelope provenance 的 dormant capture。
pub(crate) struct NativeHookSourceCapture {
    capture: ProfilerPayloadCapture,
    terminal_error: Option<String>,
    clock_admission: NativeHookClockAdmission,
}

impl NativeHookSourceCapture {
    pub(crate) fn new(options: SpoolOptions) -> Result<Self> {
        Ok(Self {
            capture: ProfilerPayloadCapture::new(protobuf_source_layout(), options)?,
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
                self.capture.append_bound_payload(
                    envelope,
                    &value,
                    append_batch_native_hook_data_root,
                )?;
                self.clock_admission
                    .observe_batch(&value, envelope.clock_id);
            }
            NativeHookRoot::Config => {
                let value: NativeHookConfig = decode_payload(envelope)?;
                self.capture.append_bound_payload(
                    envelope,
                    &value,
                    append_native_hook_config_root,
                )?;
                self.clock_admission.observe_config(&value);
            }
        }
        Ok(())
    }

    fn ensure_healthy(&self) -> Result<()> {
        if let Some(source) = &self.terminal_error {
            bail!("Native Hook Source capture is poisoned by an earlier failure: {source}");
        }
        Ok(())
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
                config_clock.envelope_clock as i32
            );
        }
        Ok(())
    }

    fn mismatched_envelope_clock(&self, config_clock: NativeHookClock) -> Option<i32> {
        let expected = config_clock.envelope_clock as i32;
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
    envelope_clock: ClockId,
}

impl NativeHookClock {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "" | "realtime" => Some(Self::new("realtime", ClockId::ClockidRealtime)),
            "mono" => Some(Self::new("mono", ClockId::ClockidMonotonic)),
            "mono_raw" => Some(Self::new("mono_raw", ClockId::ClockidMonotonicRaw)),
            "boot" => Some(Self::new("boot", ClockId::ClockidBoottime)),
            _ => None,
        }
    }

    const fn new(config_name: &'static str, envelope_clock: ClockId) -> Self {
        Self {
            config_name,
            envelope_clock,
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
