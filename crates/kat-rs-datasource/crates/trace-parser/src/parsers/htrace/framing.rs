use super::*;

impl HtraceParser {
    pub(super) fn parse_framed_file(&mut self, bytes: &[u8]) -> ParseResult<()> {
        let mut offset = 0usize;
        while offset < bytes.len() {
            if bytes.len() - offset < PROFILER_HEADER_SIZE {
                return Err(TraceEngineError::Parse(format!(
                    "truncated profiler header at byte {offset}"
                )));
            }

            let header = &bytes[offset..offset + PROFILER_HEADER_SIZE];
            let magic = read_u64_le(header, 0)?;
            if magic != PROFILER_HEADER_MAGIC {
                return Err(TraceEngineError::Parse(format!(
                    "invalid profiler header magic at byte {offset}: 0x{magic:x}"
                )));
            }

            self.add_header_clock_snapshot(header)?;
            let section_len = read_u64_le(header, 8)? as usize;
            let data_type = read_u32_le(header, 56)?;
            if section_len < PROFILER_HEADER_SIZE || offset + section_len > bytes.len() {
                return Err(TraceEngineError::Parse(format!(
                    "invalid profiler section length {section_len} at byte {offset}"
                )));
            }

            let section_end = offset + section_len;
            offset += PROFILER_HEADER_SIZE;

            if data_type != HIPROFILER_PROTOBUF_BIN {
                log::warn!(
                    target: "trace_parser::htrace",
                    "unsupported profiler section data_type={} section_len={}",
                    data_type,
                    section_len
                );
                self.tables.push_raw_event(RawEventRow {
                    ts: None,
                    cpu: None,
                    tid: None,
                    event_name: "unsupported_profiler_section".to_string(),
                    payload_json: Some(json!({ "data_type": data_type }).to_string()),
                });
                offset = section_end;
                continue;
            }

            while offset < section_end {
                if section_end - offset < SEGMENT_LENGTH_SIZE {
                    return Err(TraceEngineError::Parse(format!(
                        "truncated segment length at byte {offset}"
                    )));
                }
                let len = read_u32_le(bytes, offset)? as usize;
                offset += SEGMENT_LENGTH_SIZE;
                if offset + len > section_end {
                    return Err(TraceEngineError::Parse(format!(
                        "segment length {len} exceeds section boundary at byte {offset}"
                    )));
                }
                let segment = &bytes[offset..offset + len];
                self.parse_profiler_segment(segment)?;
                offset += len;
            }
        }
        Ok(())
    }

    pub(super) fn parse_len_prefixed_segments(&mut self, bytes: &[u8]) -> ParseResult<()> {
        let mut offset = 0usize;
        while offset < bytes.len() {
            if bytes.len() - offset < SEGMENT_LENGTH_SIZE {
                return Err(TraceEngineError::Parse(format!(
                    "truncated segment length at byte {offset}"
                )));
            }
            let len = read_u32_le(bytes, offset)? as usize;
            offset += SEGMENT_LENGTH_SIZE;
            if offset + len > bytes.len() {
                return Err(TraceEngineError::Parse(format!(
                    "segment length {len} exceeds input at byte {offset}"
                )));
            }
            self.parse_profiler_segment(&bytes[offset..offset + len])?;
            offset += len;
        }
        Ok(())
    }

    pub(super) fn parse_profiler_segment(&mut self, segment: &[u8]) -> ParseResult<()> {
        let plugin = ProfilerPluginData::decode(segment).map_err(|err| {
            TraceEngineError::Parse(format!("failed to decode ProfilerPluginData: {err}"))
        })?;
        log::trace!(
            target: "trace_parser::htrace",
            "dispatch plugin name={} data_len={} tv_sec={} tv_nsec={}",
            plugin.name,
            plugin.data.len(),
            plugin.tv_sec,
            plugin.tv_nsec
        );

        match plugin.name.as_str() {
            "ftrace-plugin" | "/data/local/tmp/libftrace_plugin.z.so" => {
                self.parse_ftrace_plugin(&plugin)
            }
            "ftrace-plugin_config" => self.parse_ftrace_plugin_config(&plugin),
            "cpu-plugin" => self.parse_cpu_plugin(&plugin),
            "diskio-plugin" => self.parse_diskio_plugin(&plugin),
            "memory-plugin" => self.parse_memory_plugin(&plugin),
            "process-plugin" => self.parse_process_plugin(&plugin),
            "arkts-plugin_config" => {
                arkts::parse_config(&plugin.data, &mut self.tables, &mut self.arkts_state)
            }
            "arkts-plugin" => {
                let ts = self.plugin_realtime_ts(&plugin);
                let monotonic_offsets = self
                    .clock_offsets
                    .get(&(TS_CLOCK_MONOTONIC, TS_CLOCK_BOOTTIME))
                    .cloned();
                arkts::parse_arkts_plugin(
                    &plugin.data,
                    ts,
                    &mut self.tables,
                    &mut self.arkts_state,
                    |src_ts| convert_clock_with_offsets(monotonic_offsets.as_ref(), src_ts),
                )
            }
            _ => {
                log::warn!(
                    target: "trace_parser::htrace",
                    "unsupported htrace plugin name={} data_len={}",
                    plugin.name,
                    plugin.data.len()
                );
                self.tables.push_raw_event(RawEventRow {
                    ts: plugin_outer_ts(&plugin),
                    cpu: None,
                    tid: None,
                    event_name: plugin.name,
                    payload_json: Some(
                        json!({
                            "status": plugin.status,
                            "clock_id": plugin.clock_id,
                            "data_len": plugin.data.len()
                        })
                        .to_string(),
                    ),
                });
                Ok(())
            }
        }
    }

    pub(super) fn parse_ftrace_plugin(&mut self, plugin: &ProfilerPluginData) -> ParseResult<()> {
        let trace = TracePluginResult::decode(plugin.data.as_slice()).map_err(|err| {
            TraceEngineError::Parse(format!("failed to decode TracePluginResult: {err}"))
        })?;

        if !self.has_clock_snapshot() {
            let snapshots = trace
                .clocks_detail
                .iter()
                .filter_map(|clock| {
                    let time = clock.time.as_ref()?;
                    let ts = u64::from(time.tv_sec)
                        .saturating_mul(1_000_000_000)
                        .saturating_add(u64::from(time.tv_nsec));
                    (ts != 0).then_some((clock.id, ts))
                })
                .collect::<Vec<_>>();
            self.add_clock_snapshot(&snapshots);
        }

        for stats in trace.ftrace_cpu_stats {
            match stats.trace_clock.as_str() {
                "boot" => self.clock_domain = "boottime".to_string(),
                "mono" => self.clock_domain = "monotonic".to_string(),
                clock if !clock.is_empty() => self.clock_domain = clock.to_string(),
                _ => {}
            }
        }

        for symbol in trace.symbols_detail {
            let symbol_name = symbol.symbol_name;
            self.symbols_by_addr
                .entry(symbol.symbol_addr)
                .or_insert_with(|| symbol_name.clone());
            if self.symbol_addrs.insert(symbol.symbol_addr) {
                self.tables.intern_string(&symbol_name);
                self.tables.push_symbol(SymbolsRow {
                    id: self.tables.next_symbol_id(),
                    funcname: symbol_name,
                    addr: symbol.symbol_addr,
                });
            }
        }

        for cpu_detail in trace.ftrace_cpu_detail {
            for event in cpu_detail.event {
                let ts = event.timestamp as i64;
                self.pending_ftrace_events.push(TimedFtraceEvent {
                    ts,
                    order: self.next_ftrace_order,
                    cpu: cpu_detail.cpu,
                    event,
                });
                self.next_ftrace_order += 1;
            }
        }

        Ok(())
    }

    pub(super) fn parse_ftrace_plugin_config(
        &mut self,
        plugin: &ProfilerPluginData,
    ) -> ParseResult<()> {
        log::debug!(
            target: "trace_parser::htrace",
            "skip known ftrace config plugin data_len={}",
            plugin.data.len()
        );
        Ok(())
    }

    pub(super) fn parse_cpu_plugin(&mut self, plugin: &ProfilerPluginData) -> ParseResult<()> {
        let data = CpuData::decode(plugin.data.as_slice())
            .map_err(|err| TraceEngineError::Parse(format!("failed to decode CpuData: {err}")))?;
        let ts = data
            .cpu_usage_info
            .as_ref()
            .and_then(|info| info.timestamp.as_ref())
            .map(sample_ts_to_ns)
            .or_else(|| plugin_outer_ts(plugin));
        let Some(ts) = ts else {
            return Ok(());
        };

        let current = CpuUsageRow {
            ts,
            dur: None,
            total_load: data.total_load,
            user_load: data.user_load,
            system_load: data.sys_load,
            process_num: data.process_num,
        };

        if let Some(mut previous) = self.pending_cpu_usage.replace(current) {
            previous.dur = Some(ts.saturating_sub(previous.ts));
            self.tables.push_cpu_usage(previous);
        }

        Ok(())
    }

    pub(super) fn parse_diskio_plugin(&mut self, plugin: &ProfilerPluginData) -> ParseResult<()> {
        let data = DiskioData::decode(plugin.data.as_slice()).map_err(|err| {
            TraceEngineError::Parse(format!("failed to decode DiskioData: {err}"))
        })?;
        let Some(prev_ts) = data.prev_timestamp.as_ref().map(collect_ts_to_ns) else {
            return Ok(());
        };
        let Some(ts) = data.timestamp.as_ref().map(collect_ts_to_ns) else {
            return Ok(());
        };
        if prev_ts == 0 || ts <= prev_ts {
            return Ok(());
        }

        let dur = ts - prev_ts;
        let rd_delta = data.rd_sectors_kb.saturating_sub(data.prev_rd_sectors_kb);
        let wr_delta = data.wr_sectors_kb.saturating_sub(data.prev_wr_sectors_kb);
        let scale = 1_000_000_000.0 / dur as f64;
        self.tables.push_diskio(DiskioRow {
            ts: prev_ts,
            dur: Some(dur),
            rd: data.rd_sectors_kb,
            wr: data.wr_sectors_kb,
            rd_speed: rd_delta as f64 * scale,
            wr_speed: wr_delta as f64 * scale,
            rd_count: data.rd_sectors_kb.saturating_mul(2),
            wr_count: data.wr_sectors_kb.saturating_mul(2),
            rd_count_speed: 0.0,
            wr_count_speed: 0.0,
        });

        Ok(())
    }

    pub(super) fn parse_memory_plugin(&mut self, plugin: &ProfilerPluginData) -> ParseResult<()> {
        let ts = self.plugin_realtime_ts(plugin);
        let mut tables = std::mem::take(&mut self.tables);
        let mut memory_state = std::mem::take(&mut self.memory_state);
        let result = memory::parse_memory_plugin(
            &plugin.data,
            ts,
            &mut tables,
            &mut memory_state,
            |sample_ts, pid, name| self.get_or_create_process(sample_ts, pid, name),
        );
        self.tables = tables;
        self.memory_state = memory_state;
        result
    }

    pub(super) fn parse_process_plugin(&mut self, plugin: &ProfilerPluginData) -> ParseResult<()> {
        process::parse_process_plugin(
            &plugin.data,
            self.plugin_realtime_ts(plugin),
            &mut self.live_process_state,
        )
    }
}
