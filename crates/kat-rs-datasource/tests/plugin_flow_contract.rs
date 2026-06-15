// plugin_flow 契约测试验证 registry 生命周期，同时保持测试代码位于 tests 目录。
use std::cell::RefCell;

use anyhow::Result;

#[allow(dead_code)]
mod proto {
    pub mod kat {
        pub mod hitrace {
            include!(concat!(env!("OUT_DIR"), "/kat.hitrace.rs"));
        }
    }

    pub(crate) use kat::hitrace::ProfilerPluginData;
}

mod domains {
    pub(crate) mod ftrace {
        #![allow(dead_code)]

        use anyhow::Result;

        use crate::{
            plugin_flow::{PluginDecoder, PluginDecoderSpec, PluginEnvelope},
            record::TraceRecordSink,
        };

        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/domains/ftrace/event.rs"
        ));

        pub(crate) const FTRACE_PLUGIN_DECODER: PluginDecoderSpec =
            PluginDecoderSpec::new(new_ftrace_plugin_decoder);

        fn new_ftrace_plugin_decoder() -> Box<dyn PluginDecoder> {
            Box::new(NoopFtraceDecoder)
        }

        struct NoopFtraceDecoder;

        impl PluginDecoder for NoopFtraceDecoder {
            fn plugin_name(&self) -> &'static str {
                "ftrace-plugin"
            }

            fn decode_data(
                &mut self,
                _envelope: &PluginEnvelope<'_>,
                _sink: &mut dyn TraceRecordSink,
            ) -> Result<()> {
                Ok(())
            }
        }
    }
}

mod record {
    #![allow(dead_code)]

    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/record.rs"));
}

mod plugin_flow {
    pub(crate) mod envelope {
        #![allow(dead_code)]

        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/plugin_flow/envelope.rs"
        ));
    }

    pub(crate) use envelope::{PluginEnvelope, PluginEnvelopeKind};

    pub(crate) mod registry {
        #![allow(dead_code)]

        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/plugin_flow/registry.rs"
        ));
    }

    pub(crate) use registry::{PluginDecoder, PluginDecoderSpec, PluginPayloadRegistry};
}

#[derive(Default)]
struct RecordingSink {
    records: Vec<record::TraceRecord>,
}

impl record::TraceRecordSink for RecordingSink {
    fn push(&mut self, record: record::TraceRecord) -> Result<()> {
        self.records.push(record);
        Ok(())
    }
}

thread_local! {
    static EVENTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

struct RecordingDecoder;

impl plugin_flow::PluginDecoder for RecordingDecoder {
    fn plugin_name(&self) -> &'static str {
        "demo-plugin"
    }

    fn configure(&mut self, envelope: &plugin_flow::PluginEnvelope<'_>) -> Result<()> {
        EVENTS.with(|events| {
            events
                .borrow_mut()
                .push(format!("configure:{}", envelope.envelope_name));
        });
        Ok(())
    }

    fn decode_data(
        &mut self,
        envelope: &plugin_flow::PluginEnvelope<'_>,
        _sink: &mut dyn record::TraceRecordSink,
    ) -> Result<()> {
        EVENTS.with(|events| {
            events
                .borrow_mut()
                .push(format!("decode_data:{}", envelope.envelope_name));
        });
        Ok(())
    }

    fn finish(&mut self, _sink: &mut dyn record::TraceRecordSink) -> Result<()> {
        EVENTS.with(|events| events.borrow_mut().push("finish".to_string()));
        Ok(())
    }
}

fn new_recording_decoder() -> Box<dyn plugin_flow::PluginDecoder> {
    Box::new(RecordingDecoder)
}

fn plugin_message(name: &str) -> proto::ProfilerPluginData {
    proto::ProfilerPluginData {
        name: name.to_string(),
        ..Default::default()
    }
}

fn reset_events() {
    EVENTS.with(|events| events.borrow_mut().clear());
}

fn recorded_events() -> Vec<String> {
    EVENTS.with(|events| events.borrow().clone())
}

#[test]
fn registry_dispatches_config_data_and_finish_to_matching_decoder() {
    reset_events();
    let specs = [plugin_flow::PluginDecoderSpec::new(new_recording_decoder)];
    let mut registry = plugin_flow::PluginPayloadRegistry::new(&specs);
    let mut sink = RecordingSink::default();

    let config = plugin_message("demo-plugin_config");
    let config = plugin_flow::PluginEnvelope::from_profiler_plugin_data(&config, 10);
    registry
        .dispatch(&config, &mut sink)
        .expect("config dispatch");

    let data = plugin_message("demo-plugin");
    let data = plugin_flow::PluginEnvelope::from_profiler_plugin_data(&data, 20);
    registry.dispatch(&data, &mut sink).expect("data dispatch");

    let unknown = plugin_message("unknown-plugin");
    let unknown = plugin_flow::PluginEnvelope::from_profiler_plugin_data(&unknown, 30);
    registry
        .dispatch(&unknown, &mut sink)
        .expect("unknown dispatch");

    registry.finish(&mut sink).expect("finish dispatch");

    assert_eq!(
        recorded_events(),
        vec![
            "configure:demo-plugin_config".to_string(),
            "decode_data:demo-plugin".to_string(),
            "finish".to_string(),
        ]
    );
}
