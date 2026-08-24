use std::cell::RefCell;

use anyhow::Result;

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
        pub(crate) enum NativeHookRecord {}
    }
}

mod record {
    #![allow(dead_code)]

    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/record.rs"));
}

mod profiler {
    pub(crate) mod envelope {
        #![allow(dead_code)]

        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/formats/hitrace/profiler/envelope.rs"
        ));
    }

    pub(crate) use envelope::{PluginEnvelope, PluginEnvelopeKind};

    pub(crate) mod registry {
        #![allow(dead_code)]

        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/formats/hitrace/profiler/registry.rs"
        ));
    }

    pub(crate) use registry::{PluginDecoder, PluginDecoderSpec, PluginPayloadRegistry};
}

struct RecordingSink;

impl record::TraceRecordSink for RecordingSink {
    fn push(&mut self, _record: record::TraceRecord) -> Result<()> {
        Ok(())
    }
}

thread_local! {
    static EVENTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
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
    let mut sink = RecordingSink;

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
