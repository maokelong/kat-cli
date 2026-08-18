use std::cell::{Cell, RefCell};

use anyhow::Result;
use prost::Message;

#[allow(dead_code)]
mod proto {
    pub mod kat {
        pub mod hitrace {
            include!(concat!(env!("OUT_DIR"), "/kat.hitrace.rs"));
        }

        pub mod native_hook {
            include!(concat!(env!("OUT_DIR"), "/kat.native_hook.rs"));
        }
    }

    pub(crate) use kat::hitrace::ProfilerPluginData;
}

mod domains {
    pub(crate) mod ftrace {
        #[allow(dead_code)]
        pub(crate) enum FtraceCaptureRecord {}

        #[allow(dead_code)]
        pub(crate) enum FtraceRecord {}

        #[allow(dead_code)]
        pub(crate) struct FtraceEventRecord;
    }

    pub(crate) mod native_hook {
        pub(crate) enum NativeHookRecord {
            Decoded,
        }
    }
}

mod record {
    #![allow(dead_code)]

    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/record.rs"));
}

mod profiler {
    pub(crate) mod claim {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/formats/hitrace/profiler/claim.rs"
        ));
    }

    pub(crate) mod envelope {
        #![allow(dead_code)]

        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/formats/hitrace/profiler/envelope.rs"
        ));
    }

    pub(crate) use envelope::{PluginEnvelope, PluginEnvelopeKind};

    pub(crate) mod payload {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/formats/hitrace/profiler/payload.rs"
        ));
    }

    pub(crate) use payload::decode_payload;

    pub(crate) mod registry {
        #![allow(dead_code)]

        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/formats/hitrace/profiler/registry.rs"
        ));
    }

    pub(crate) use registry::{PluginDecoder, PluginDecoderSpec, PluginPayloadRegistry};

    pub(crate) use claim::dispatch_plugin_envelope;
}

#[derive(Default)]
struct RecordingSink {
    native_hook_records: usize,
}

impl record::TraceRecordSink for RecordingSink {
    fn push(&mut self, record: record::TraceRecord) -> Result<()> {
        if matches!(record, record::TraceRecord::NativeHook(_)) {
            self.native_hook_records += 1;
        }
        Ok(())
    }
}

thread_local! {
    static EVENTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static CLAIMANT_CALLS: Cell<usize> = const { Cell::new(0) };
    static CLAIMANT_TYPED_DECODES: Cell<usize> = const { Cell::new(0) };
    static LEGACY_TYPED_DECODES: Cell<usize> = const { Cell::new(0) };
}

struct RecordingDecoder;

impl profiler::PluginDecoder for RecordingDecoder {
    fn plugin_name(&self) -> &'static str {
        "demo-plugin"
    }

    fn configure(
        &mut self,
        envelope: &profiler::PluginEnvelope<'_>,
        _sink: &mut dyn record::TraceRecordSink,
    ) -> Result<()> {
        EVENTS.with(|events| {
            events
                .borrow_mut()
                .push(format!("configure:{}", envelope.envelope_name))
        });
        Ok(())
    }

    fn decode_data(
        &mut self,
        envelope: &profiler::PluginEnvelope<'_>,
        _sink: &mut dyn record::TraceRecordSink,
    ) -> Result<()> {
        EVENTS.with(|events| {
            events
                .borrow_mut()
                .push(format!("decode_data:{}", envelope.envelope_name))
        });
        Ok(())
    }

    fn finish(&mut self, _sink: &mut dyn record::TraceRecordSink) -> Result<()> {
        EVENTS.with(|events| events.borrow_mut().push("finish".to_string()));
        Ok(())
    }
}

fn new_recording_decoder() -> Box<dyn profiler::PluginDecoder> {
    Box::new(RecordingDecoder)
}

struct LegacyNativeHookDecoder;

impl profiler::PluginDecoder for LegacyNativeHookDecoder {
    fn plugin_name(&self) -> &'static str {
        "nativehook"
    }

    fn decode_data(
        &mut self,
        envelope: &profiler::PluginEnvelope<'_>,
        sink: &mut dyn record::TraceRecordSink,
    ) -> Result<()> {
        let _: proto::kat::native_hook::BatchNativeHookData = profiler::decode_payload(envelope)?;
        LEGACY_TYPED_DECODES.with(|count| count.set(count.get() + 1));
        sink.push(record::TraceRecord::NativeHook(Box::new(
            domains::native_hook::NativeHookRecord::Decoded,
        )))
    }
}

fn new_legacy_native_hook_decoder() -> Box<dyn profiler::PluginDecoder> {
    Box::new(LegacyNativeHookDecoder)
}

fn plugin_message(name: &str) -> proto::ProfilerPluginData {
    proto::ProfilerPluginData {
        name: name.to_string(),
        ..Default::default()
    }
}

#[test]
fn registry_dispatches_config_data_and_finish_to_matching_decoder() {
    EVENTS.with(|events| events.borrow_mut().clear());
    let specs = [profiler::PluginDecoderSpec::new(new_recording_decoder)];
    let mut registry = profiler::PluginPayloadRegistry::new(&specs);
    let mut sink = RecordingSink::default();

    let config = plugin_message("demo-plugin_config");
    let config = profiler::PluginEnvelope::from_profiler_plugin_data(&config, 10);
    registry
        .dispatch(&config, &mut sink)
        .expect("config dispatch");

    let data = plugin_message("demo-plugin");
    let data = profiler::PluginEnvelope::from_profiler_plugin_data(&data, 20);
    registry.dispatch(&data, &mut sink).expect("data dispatch");

    let unknown = plugin_message("unknown-plugin");
    let unknown = profiler::PluginEnvelope::from_profiler_plugin_data(&unknown, 30);
    registry
        .dispatch(&unknown, &mut sink)
        .expect("unknown dispatch");

    registry.finish(&mut sink).expect("finish dispatch");

    assert_eq!(
        EVENTS.with(|events| events.borrow().clone()),
        vec![
            "configure:demo-plugin_config".to_string(),
            "decode_data:demo-plugin".to_string(),
            "finish".to_string(),
        ]
    );
}

#[test]
fn claimed_payload_is_typed_decoded_once_and_never_reaches_legacy_decoder() {
    reset_claim_counters();
    let specs = [profiler::PluginDecoderSpec::new(
        new_legacy_native_hook_decoder,
    )];
    let mut registry = profiler::PluginPayloadRegistry::new(&specs);
    let mut sink = RecordingSink::default();
    let message = native_hook_message();
    let envelope = profiler::PluginEnvelope::from_profiler_plugin_data(&message, 1_024);
    let mut claimant = |envelope: &profiler::PluginEnvelope<'_>| {
        CLAIMANT_CALLS.with(|count| count.set(count.get() + 1));
        let _: proto::kat::native_hook::BatchNativeHookData = profiler::decode_payload(envelope)?;
        CLAIMANT_TYPED_DECODES.with(|count| count.set(count.get() + 1));
        Ok(true)
    };

    let known =
        profiler::dispatch_plugin_envelope(&envelope, &mut registry, &mut sink, &mut claimant)
            .expect("bound Native Hook payload is claimed");

    assert!(known);
    assert_eq!(CLAIMANT_CALLS.with(Cell::get), 1);
    assert_eq!(CLAIMANT_TYPED_DECODES.with(Cell::get), 1);
    assert_eq!(LEGACY_TYPED_DECODES.with(Cell::get), 0);
    assert_eq!(sink.native_hook_records, 0);
}

#[test]
fn unclaimed_payload_reaches_legacy_decoder_once() {
    reset_claim_counters();
    let specs = [profiler::PluginDecoderSpec::new(
        new_legacy_native_hook_decoder,
    )];
    let mut registry = profiler::PluginPayloadRegistry::new(&specs);
    let mut sink = RecordingSink::default();
    let message = native_hook_message();
    let envelope = profiler::PluginEnvelope::from_profiler_plugin_data(&message, 2_048);
    let mut claimant = |_envelope: &profiler::PluginEnvelope<'_>| {
        CLAIMANT_CALLS.with(|count| count.set(count.get() + 1));
        Ok(false)
    };

    let known =
        profiler::dispatch_plugin_envelope(&envelope, &mut registry, &mut sink, &mut claimant)
            .expect("unclaimed Native Hook payload falls back to the registry");

    assert!(known);
    assert_eq!(CLAIMANT_CALLS.with(Cell::get), 1);
    assert_eq!(CLAIMANT_TYPED_DECODES.with(Cell::get), 0);
    assert_eq!(LEGACY_TYPED_DECODES.with(Cell::get), 1);
    assert_eq!(sink.native_hook_records, 1);
}

fn native_hook_message() -> proto::ProfilerPluginData {
    proto::ProfilerPluginData {
        name: "nativehook".to_owned(),
        data: proto::kat::native_hook::BatchNativeHookData::default().encode_to_vec(),
        ..Default::default()
    }
}

fn reset_claim_counters() {
    CLAIMANT_CALLS.with(|count| count.set(0));
    CLAIMANT_TYPED_DECODES.with(|count| count.set(0));
    LEGACY_TYPED_DECODES.with(|count| count.set(0));
}
