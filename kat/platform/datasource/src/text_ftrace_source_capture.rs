use std::collections::BTreeMap;

use anyhow::{Context, Result};

use crate::{
    generated_profiler_source_emitter::{
        append_trace_plugin_config_root_without_parent,
        append_trace_plugin_result_ftrace_cpu_detail_event_subtree,
        append_trace_plugin_result_ftrace_cpu_detail_subtree,
        append_trace_plugin_result_incremental_root, protobuf_source_layout,
    },
    proto::{
        TracePluginConfig, TracePluginResult,
        kat::hitrace::{FtraceCpuDetailMsg, FtraceEvent},
    },
    protobuf_source::{PreparedSourceTables, SourceTableCapture, SpoolOptions},
};

pub(crate) struct TextFtraceSourceCapture {
    capture: SourceTableCapture,
    root_row_id: u64,
    cpu_details: BTreeMap<u32, TextFtraceCpuDetail>,
    next_cpu_index: u64,
}

struct TextFtraceCpuDetail {
    row_id: u64,
    next_event_index: u64,
}

impl TextFtraceSourceCapture {
    pub(crate) fn new(options: SpoolOptions, config: &TracePluginConfig) -> Result<Self> {
        let mut capture = protobuf_source_layout().into_capture(options)?;
        let root_row_id = append_trace_plugin_result_incremental_root(
            &mut capture,
            None,
            &TracePluginResult::default(),
        )?;
        append_trace_plugin_config_root_without_parent(&mut capture, config)?;
        Ok(Self {
            capture,
            root_row_id,
            cpu_details: BTreeMap::new(),
            next_cpu_index: 0,
        })
    }

    pub(crate) fn append_event(&mut self, cpu: u32, event: &FtraceEvent) -> Result<()> {
        if !self.cpu_details.contains_key(&cpu) {
            let repeated_index = self.next_cpu_index;
            self.next_cpu_index = self
                .next_cpu_index
                .checked_add(1)
                .context("text ftrace CPU repeated index overflows")?;
            let row_id = append_trace_plugin_result_ftrace_cpu_detail_subtree(
                &mut self.capture,
                self.root_row_id,
                repeated_index,
                &FtraceCpuDetailMsg {
                    cpu,
                    event: Vec::new(),
                    overwrite: None,
                },
            )?;
            self.cpu_details.insert(
                cpu,
                TextFtraceCpuDetail {
                    row_id,
                    next_event_index: 0,
                },
            );
        }
        let detail = self
            .cpu_details
            .get_mut(&cpu)
            .expect("text ftrace CPU detail was initialized");
        let repeated_index = detail.next_event_index;
        detail.next_event_index = detail
            .next_event_index
            .checked_add(1)
            .context("text ftrace event repeated index overflows")?;
        append_trace_plugin_result_ftrace_cpu_detail_event_subtree(
            &mut self.capture,
            detail.row_id,
            repeated_index,
            event,
        )?;
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<PreparedSourceTables> {
        self.capture.finish()
    }
}
