use crate::TraceEngineError;
use crate::{HarmonyTraceParser, ParseResult};
use serde_json::json;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;
use trace_model::{
    ParsedTrace, PerfCallchainRow, PerfFilesRow, PerfReportRow, PerfSampleRow, PerfThreadRow,
    RawEventRow, TraceTableBuilder,
};

const PERF_MAGIC: &[u8; 8] = b"PERFILE2";
const PERF_FILE_HEADER_MIN_SIZE: usize = 104;

const PERF_RECORD_MMAP: u32 = 1;
const PERF_RECORD_COMM: u32 = 3;
const PERF_RECORD_SAMPLE: u32 = 9;
const PERF_RECORD_MMAP2: u32 = 10;
const PERF_RECORD_AUXTRACE: u32 = 71;

const PERF_SAMPLE_IP: u64 = 1 << 0;
const PERF_SAMPLE_TID: u64 = 1 << 1;
const PERF_SAMPLE_TIME: u64 = 1 << 2;
const PERF_SAMPLE_ADDR: u64 = 1 << 3;
const PERF_SAMPLE_READ: u64 = 1 << 4;
const PERF_SAMPLE_CALLCHAIN: u64 = 1 << 5;
const PERF_SAMPLE_ID: u64 = 1 << 6;
const PERF_SAMPLE_CPU: u64 = 1 << 7;
const PERF_SAMPLE_PERIOD: u64 = 1 << 8;
const PERF_SAMPLE_STREAM_ID: u64 = 1 << 9;
const PERF_SAMPLE_RAW: u64 = 1 << 10;
const PERF_SAMPLE_BRANCH_STACK: u64 = 1 << 11;
const PERF_SAMPLE_REGS_USER: u64 = 1 << 12;
const PERF_SAMPLE_STACK_USER: u64 = 1 << 13;
const PERF_SAMPLE_IDENTIFIER: u64 = 1 << 16;
const PERF_SAMPLE_SERVER_PID: u64 = 1 << 31;

const FEATURE_CMDLINE: u32 = 11;
const FEATURE_EVENT_DESC: u32 = 12;
const FEATURE_HIPERF_FILES_SYMBOL: u32 = 192;
const FEATURE_HIPERF_WORKLOAD_CMD: u32 = 193;
const FEATURE_HIPERF_FILES_UNISTACK_TABLE: u32 = 197;

const STACK_ID_INDEX_BITS: u64 = 23;
const STACK_ID_INDEX_MASK: u64 = (1 << STACK_ID_INDEX_BITS) - 1;
const NODE_IP_BITS: u64 = 40;
const NODE_IP_MASK: u64 = (1 << NODE_IP_BITS) - 1;
const NODE_PREV_MASK: u64 = (1 << 23) - 1;
const KERNEL_PREFIX: u64 = 0x00ff_ffff_0000_0000;
const PERF_CONTEXT_MAX: u64 = u64::MAX - 4095;
const BAD_IP_ADDRESS: u64 = 2;

#[derive(Default)]
pub struct PerfParser {
    tables: TraceTableBuilder,
    input_hash: u64,
    start_ts: Option<i64>,
    end_ts: Option<i64>,
}

#[derive(Debug, Clone)]
struct PerfHeader {
    header_size: u64,
    attr_size: u64,
    attrs_offset: u64,
    attrs_size: u64,
    data_offset: u64,
    data_size: u64,
    event_types_offset: u64,
    event_types_size: u64,
    features: Vec<u32>,
}

#[derive(Debug, Clone, Default)]
struct PerfAttr {
    sample_type: u64,
    sample_regs_user: u64,
}

#[derive(Debug, Clone, Default)]
struct PerfFeatureState {
    event_config_by_id: HashMap<u64, u64>,
    event_names_by_index: Vec<String>,
    path_to_file_id: HashMap<String, u64>,
    symbols_by_file_id: HashMap<u64, Vec<PerfSymbol>>,
    stack_tables: HashMap<u32, HashMap<u32, u64>>,
    has_stack_table: bool,
}

#[derive(Debug, Clone)]
struct PerfSymbol {
    serial_id: u32,
    vaddr: u64,
    len: u32,
    name: String,
}

#[derive(Debug, Clone)]
struct PerfMap {
    start: u64,
    end: u64,
    pgoff: u64,
    file_id: Option<u64>,
}

impl PerfMap {
    fn contains(&self, ip: u64) -> bool {
        ip >= self.start && ip < self.end
    }
}

#[derive(Debug, Clone, Default)]
struct PerfRuntimeMaps {
    by_tid: HashMap<u32, Vec<PerfMap>>,
    by_pid: HashMap<u32, Vec<PerfMap>>,
    kernel: Vec<PerfMap>,
}

#[derive(Debug, Clone, Default)]
struct ResolvedFrame {
    file_id: Option<u64>,
    symbol_id: Option<u64>,
    name: Option<String>,
    vaddr_in_file: Option<u64>,
    offset_to_vaddr: Option<u64>,
}

#[derive(Debug, Clone, Default)]
struct PerfSample {
    ip: Option<u64>,
    pid: u32,
    tid: u32,
    time: u64,
    id: u64,
    cpu: u32,
    period: u64,
    frames: Vec<u64>,
}

impl PerfParser {
    pub fn new() -> Self {
        Self::default()
    }

    fn reset_for_input(&mut self, bytes: &[u8]) {
        *self = Self::new();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hasher);
        self.input_hash = hasher.finish();
        self.tables.push_metadata("parser", Some("trace-parser"));
        self.tables.push_metadata("parser_version", Some("0.1.0"));
        self.tables.push_metadata("source_format", Some("perf"));
    }

    fn parse_perf(&mut self, bytes: &[u8]) -> ParseResult<()> {
        if !looks_like_perf(bytes) {
            return Err(TraceEngineError::Parse(
                "not a perf file: missing PERFILE2 magic".to_string(),
            ));
        }
        if bytes.len() < PERF_FILE_HEADER_MIN_SIZE {
            return Err(TraceEngineError::Parse(format!(
                "truncated perf header: {} bytes",
                bytes.len()
            )));
        }

        let header = PerfHeader {
            header_size: read_u64_le(bytes, 8)?,
            attr_size: read_u64_le(bytes, 16)?,
            attrs_offset: read_u64_le(bytes, 24)?,
            attrs_size: read_u64_le(bytes, 32)?,
            data_offset: read_u64_le(bytes, 40)?,
            data_size: read_u64_le(bytes, 48)?,
            event_types_offset: read_u64_le(bytes, 56)?,
            event_types_size: read_u64_le(bytes, 64)?,
            features: parse_features(bytes.get(72..104).unwrap_or_default()),
        };

        let attrs = self.parse_attrs(bytes, &header)?;
        let mut report_id = 0u64;
        let mut features = PerfFeatureState::default();

        self.tables.push_raw_event(RawEventRow {
            ts: None,
            cpu: None,
            tid: None,
            event_name: "perf_header".to_string(),
            payload_json: Some(
                json!({
                    "file_size": bytes.len(),
                    "header_size": header.header_size,
                    "attr_size": header.attr_size,
                    "attrs": { "offset": header.attrs_offset, "size": header.attrs_size },
                    "data": { "offset": header.data_offset, "size": header.data_size },
                    "event_types": { "offset": header.event_types_offset, "size": header.event_types_size },
                    "features": header.features
                })
                .to_string(),
            ),
        });

        self.parse_feature_sections(bytes, &header, &mut features, &mut report_id)?;
        self.scan_data_section(bytes, &header, attrs.first(), &features);
        Ok(())
    }

    fn parse_attrs(&self, bytes: &[u8], header: &PerfHeader) -> ParseResult<Vec<PerfAttr>> {
        if header.attr_size == 0 {
            return Ok(Vec::new());
        }

        let attr_count = header.attrs_size / header.attr_size;
        let mut attrs = Vec::with_capacity(attr_count as usize);
        for index in 0..attr_count {
            let start = header
                .attrs_offset
                .saturating_add(index.saturating_mul(header.attr_size))
                as usize;
            let end = start.saturating_add(header.attr_size as usize);
            let Some(attr_bytes) = bytes.get(start..end) else {
                break;
            };

            let sample_type = read_u64_le(attr_bytes, 24).unwrap_or(0);
            let sample_regs_user = read_u64_le(attr_bytes, 80).unwrap_or(0);

            attrs.push(PerfAttr {
                sample_type,
                sample_regs_user,
            });
        }
        Ok(attrs)
    }

    fn parse_feature_sections(
        &mut self,
        bytes: &[u8],
        header: &PerfHeader,
        state: &mut PerfFeatureState,
        report_id: &mut u64,
    ) -> ParseResult<()> {
        let mut section_header_offset =
            header.data_offset.saturating_add(header.data_size) as usize;
        let mut deferred_cmdline = None;
        let mut deferred_workload = None;
        for feature in &header.features {
            let Some(section_header) = bytes.get(section_header_offset..section_header_offset + 16)
            else {
                break;
            };
            let section_offset = read_u64_le(section_header, 0)? as usize;
            let section_size = read_u64_le(section_header, 8)? as usize;
            section_header_offset += 16;

            let Some(section) =
                bytes.get(section_offset..section_offset.saturating_add(section_size))
            else {
                continue;
            };

            match *feature {
                FEATURE_EVENT_DESC => self.parse_event_desc(section, state, report_id),
                FEATURE_HIPERF_FILES_SYMBOL => self.parse_files_symbol(section, state),
                FEATURE_CMDLINE => {
                    deferred_cmdline = read_perf_string_at(section, 0).map(|(value, _)| value);
                }
                FEATURE_HIPERF_WORKLOAD_CMD => {
                    deferred_workload = read_perf_string_at(section, 0).map(|(value, _)| value);
                }
                FEATURE_HIPERF_FILES_UNISTACK_TABLE => {
                    self.parse_unistack_table(section, state);
                }
                _ => {}
            }
        }
        if let Some(workload) = deferred_workload {
            if !workload.is_empty() {
                self.push_report(report_id, "workload_cmd", workload);
            }
        }
        if let Some(cmdline) = deferred_cmdline {
            self.push_report(report_id, "cmdline", cmdline);
        }
        Ok(())
    }

    fn parse_event_desc(
        &mut self,
        section: &[u8],
        state: &mut PerfFeatureState,
        report_id: &mut u64,
    ) {
        let mut offset = 0usize;
        let Some(nr) = read_u32_le(section, offset).ok() else {
            return;
        };
        offset += 4;
        let Some(attr_size) = read_u32_le(section, offset).ok().map(|v| v as usize) else {
            return;
        };
        offset += 4;

        for _ in 0..nr {
            if offset.saturating_add(attr_size) > section.len() {
                return;
            }
            offset += attr_size;

            let Some(nr_ids) = read_u32_le(section, offset).ok().map(|v| v as usize) else {
                return;
            };
            offset += 4;

            let Some((name, next_offset)) = read_perf_string_at(section, offset) else {
                return;
            };
            offset = next_offset;

            let config_index = state.event_names_by_index.len() as u64;
            for _ in 0..nr_ids {
                let Some(id) = read_u64_le(section, offset).ok() else {
                    return;
                };
                offset += 8;
                state.event_config_by_id.insert(id, config_index);
            }
            state.event_names_by_index.push(name.clone());
            self.push_report(report_id, "config_name", name);
        }
    }

    fn parse_files_symbol(&mut self, section: &[u8], state: &mut PerfFeatureState) {
        let mut offset = 0usize;
        let Some(file_count) = read_u32_le(section, offset).ok() else {
            return;
        };
        offset += 4;

        for file_id in 0..file_count as u64 {
            let Some((path, next_offset)) = read_perf_string_at(section, offset) else {
                return;
            };
            offset = next_offset;

            if offset + 4 + 8 + 8 > section.len() {
                return;
            }
            offset += 4; // symbol_type
            offset += 8; // text_exec_vaddr
            offset += 8; // text_exec_vaddr_file_offset

            let Some((_build_id, next_offset)) = read_perf_string_at(section, offset) else {
                return;
            };
            offset = next_offset;

            let Some(symbol_count) = read_u32_le(section, offset).ok() else {
                return;
            };
            offset += 4;

            state.path_to_file_id.insert(path.clone(), file_id);

            if symbol_count == 0 {
                let id = self.tables.next_perf_file_id();
                self.tables.push_perf_file(PerfFilesRow {
                    id,
                    file_id,
                    serial_id: Some(u32::MAX),
                    symbol: None,
                    path: Some(path),
                });
                continue;
            }

            for serial in 0..symbol_count {
                if offset + 8 + 4 > section.len() {
                    return;
                }
                let vaddr = read_u64_le(section, offset).unwrap_or(0);
                offset += 8;
                let len = read_u32_le(section, offset).unwrap_or(0);
                offset += 4;
                let Some((symbol, next_offset)) = read_perf_string_at(section, offset) else {
                    return;
                };
                offset = next_offset;

                state
                    .symbols_by_file_id
                    .entry(file_id)
                    .or_default()
                    .push(PerfSymbol {
                        serial_id: serial,
                        vaddr,
                        len,
                        name: symbol.clone(),
                    });

                let id = self.tables.next_perf_file_id();
                self.tables.push_perf_file(PerfFilesRow {
                    id,
                    file_id,
                    serial_id: Some(serial),
                    symbol: Some(symbol),
                    path: Some(path.clone()),
                });
            }
        }
    }

    fn parse_unistack_table(&mut self, section: &[u8], state: &mut PerfFeatureState) {
        let mut offset = 0usize;
        let Some(table_count) = read_u32_le(section, offset).ok() else {
            return;
        };
        offset += 4;
        state.has_stack_table = true;

        for _ in 0..table_count {
            if offset + 12 > section.len() {
                return;
            }
            let pid = read_u32_le(section, offset).unwrap_or(0);
            offset += 4;
            let _table_size = read_u32_le(section, offset).unwrap_or(0);
            offset += 4;
            let node_count = read_u32_le(section, offset).unwrap_or(0);
            offset += 4;

            let table = state.stack_tables.entry(pid).or_default();
            for _ in 0..node_count {
                if offset + 12 > section.len() {
                    return;
                }
                let index = read_u32_le(section, offset).unwrap_or(0);
                offset += 4;
                let node = read_u64_le(section, offset).unwrap_or(0);
                offset += 8;
                table.insert(index, node);
            }
        }
    }

    fn scan_data_section(
        &mut self,
        bytes: &[u8],
        header: &PerfHeader,
        attr: Option<&PerfAttr>,
        features: &PerfFeatureState,
    ) {
        let start = header.data_offset as usize;
        let end = start
            .saturating_add(header.data_size as usize)
            .min(bytes.len());
        if start >= bytes.len() || start >= end {
            self.tables.push_raw_event(RawEventRow {
                ts: None,
                cpu: None,
                tid: None,
                event_name: "perf_data_section".to_string(),
                payload_json: Some(
                    json!({
                        "status": "missing_or_empty",
                        "offset": header.data_offset,
                        "size": header.data_size
                    })
                    .to_string(),
                ),
            });
            return;
        }

        let mut offset = start;
        let mut counts = BTreeMap::<u32, u64>::new();
        let mut sample_count = 0u64;
        let mut record_count = 0u64;
        let mut seen_threads = HashSet::<(u32, u32)>::new();
        let mut callchain_ids = HashMap::<(u32, Vec<u64>), u32>::new();
        let mut next_callchain_id = 1u32;
        let mut maps = PerfRuntimeMaps::default();

        while offset + 8 <= end {
            let Some(record_type) = read_u32_le(bytes, offset).ok() else {
                break;
            };
            let Some(misc) = read_u16_le(bytes, offset + 4).ok() else {
                break;
            };
            let Some(size) = read_u16_le(bytes, offset + 6)
                .ok()
                .map(|size| size as usize)
            else {
                break;
            };
            if size < 8 || offset + size > end {
                self.tables.push_raw_event(RawEventRow {
                    ts: None,
                    cpu: None,
                    tid: None,
                    event_name: "truncated_perf_record".to_string(),
                    payload_json: Some(
                        json!({
                            "offset": offset,
                            "record_type": record_type,
                            "record_size": size,
                            "data_end": end
                        })
                        .to_string(),
                    ),
                });
                break;
            }

            *counts.entry(record_type).or_default() += 1;
            record_count += 1;
            if record_count <= 100 {
                self.tables.push_raw_event(RawEventRow {
                    ts: None,
                    cpu: None,
                    tid: None,
                    event_name: "perf_record".to_string(),
                    payload_json: Some(
                        json!({
                            "offset": offset,
                            "record_type": record_type,
                            "misc": misc,
                            "size": size,
                            "name": perf_record_name(record_type)
                        })
                        .to_string(),
                    ),
                });
            }

            let record = &bytes[offset..offset + size];
            match record_type {
                PERF_RECORD_MMAP | PERF_RECORD_MMAP2 => {
                    self.parse_mmap_record(record, record_type, misc, features, &mut maps);
                }
                PERF_RECORD_COMM => self.parse_comm_record(record, &mut seen_threads),
                PERF_RECORD_SAMPLE => {
                    sample_count += 1;
                    if let Some(attr) = attr {
                        if let Some(sample) = self.parse_sample_record(record, attr, features) {
                            self.append_sample(
                                sample,
                                features,
                                &maps,
                                &mut callchain_ids,
                                &mut next_callchain_id,
                            );
                        }
                    }
                }
                PERF_RECORD_AUXTRACE => {
                    let spe_size = read_u64_le(record, 8).unwrap_or(0) as usize;
                    offset = offset.saturating_add(size).saturating_add(spe_size);
                    continue;
                }
                _ => {}
            }

            offset += size;
        }

        self.tables.push_raw_event(RawEventRow {
            ts: None,
            cpu: None,
            tid: None,
            event_name: "perf_data_records".to_string(),
            payload_json: Some(
                json!({
                    "records": record_count,
                    "samples": sample_count,
                    "by_type": counts,
                    "first_raw_event_records": record_count.min(100)
                })
                .to_string(),
            ),
        });
    }

    fn parse_mmap_record(
        &self,
        record: &[u8],
        record_type: u32,
        misc: u16,
        features: &PerfFeatureState,
        maps: &mut PerfRuntimeMaps,
    ) {
        let Some((pid, tid, start, len, pgoff, path)) = parse_mmap_payload(record, record_type)
        else {
            return;
        };
        let map = PerfMap {
            start,
            end: start.saturating_add(len),
            pgoff,
            file_id: resolve_file_id(features, &path),
        };
        if misc & 1 != 0 {
            maps.kernel.push(map.clone());
        }
        maps.by_tid.entry(tid).or_default().push(map.clone());
        maps.by_pid.entry(pid).or_default().push(map);
    }

    fn parse_comm_record(&mut self, record: &[u8], seen_threads: &mut HashSet<(u32, u32)>) {
        if record.len() < 16 {
            return;
        }
        let pid = read_u32_le(record, 8).unwrap_or(0);
        let tid = read_u32_le(record, 12).unwrap_or(0);
        if !seen_threads.insert((tid, pid)) {
            return;
        }
        let comm = read_c_string(&record[16..]);
        let id = self.tables.next_perf_thread_id();
        self.tables.push_perf_thread(PerfThreadRow {
            id,
            thread_id: tid,
            process_id: pid,
            thread_name: empty_to_none(comm),
        });
    }

    fn parse_sample_record(
        &self,
        record: &[u8],
        attr: &PerfAttr,
        features: &PerfFeatureState,
    ) -> Option<PerfSample> {
        let sample_type = attr.sample_type;
        let mut offset = 8usize;
        let mut sample = PerfSample::default();

        let _sample_id = pop_u64(
            record,
            &mut offset,
            sample_type & PERF_SAMPLE_IDENTIFIER != 0,
        )?;
        sample.ip = pop_u64(record, &mut offset, sample_type & PERF_SAMPLE_IP != 0)?;
        if sample_type & PERF_SAMPLE_TID != 0 {
            sample.pid = read_u32_le(record, offset).ok()?;
            sample.tid = read_u32_le(record, offset + 4).ok()?;
            offset += 8;
        }
        sample.time =
            pop_u64(record, &mut offset, sample_type & PERF_SAMPLE_TIME != 0)?.unwrap_or(0);
        let _addr = pop_u64(record, &mut offset, sample_type & PERF_SAMPLE_ADDR != 0)?;
        sample.id = pop_u64(record, &mut offset, sample_type & PERF_SAMPLE_ID != 0)?.unwrap_or(0);
        let _stream_id = pop_u64(
            record,
            &mut offset,
            sample_type & PERF_SAMPLE_STREAM_ID != 0,
        )?;
        if sample_type & PERF_SAMPLE_CPU != 0 {
            sample.cpu = read_u32_le(record, offset).ok()?;
            offset += 8;
        }
        sample.period =
            pop_u64(record, &mut offset, sample_type & PERF_SAMPLE_PERIOD != 0)?.unwrap_or(0);

        if sample_type & PERF_SAMPLE_READ != 0 {
            return None;
        }

        let mut callchain_nr = 0u64;
        if sample_type & PERF_SAMPLE_CALLCHAIN != 0 {
            callchain_nr = read_u64_le(record, offset).ok()?;
            offset += 8;
            let frame_bytes = callchain_nr.checked_mul(8)? as usize;
            if offset + frame_bytes > record.len() {
                return None;
            }
            for frame_offset in (offset..offset + frame_bytes).step_by(8) {
                sample.frames.push(read_u64_le(record, frame_offset).ok()?);
            }
            offset += frame_bytes;
        }

        if sample_type & PERF_SAMPLE_RAW != 0 {
            let raw_size = read_u32_le(record, offset).ok()? as usize;
            offset += 4;
            if offset + raw_size > record.len() {
                return None;
            }
            offset += raw_size;
        }
        if sample_type & PERF_SAMPLE_BRANCH_STACK != 0 {
            let branch_nr = read_u64_le(record, offset).ok()? as usize;
            offset += 8;
            let branch_bytes = branch_nr.checked_mul(24)?;
            if offset + branch_bytes > record.len() {
                return None;
            }
            offset += branch_bytes;
        }
        if sample_type & PERF_SAMPLE_REGS_USER != 0 {
            let user_abi = read_u64_le(record, offset).ok()?;
            offset += 8;
            if user_abi > 0 {
                let reg_bytes = attr.sample_regs_user.count_ones() as usize * 8;
                if offset + reg_bytes > record.len() {
                    return None;
                }
                offset += reg_bytes;
            }
        }
        if sample_type & PERF_SAMPLE_SERVER_PID != 0 {
            let server_nr = read_u64_le(record, offset).ok()? as usize;
            offset += 8;
            let server_bytes = server_nr.checked_mul(8)?;
            if offset + server_bytes > record.len() {
                return None;
            }
            offset += server_bytes;
        }
        if sample_type & PERF_SAMPLE_STACK_USER != 0 {
            let stack_size = read_u64_le(record, offset).ok()? as usize;
            offset += 8;
            if offset + stack_size > record.len() {
                return None;
            }
            offset += stack_size;
            if stack_size > 0 {
                let _dyn_size = read_u64_le(record, offset).ok()?;
                offset += 8;
            }
        }

        if callchain_nr == 0 && features.has_stack_table && record.len().saturating_sub(offset) == 8
        {
            let stack_id = read_u64_le(record, offset).ok()?;
            if let Some(frames) = recover_stack_frames(features, sample.pid, stack_id) {
                sample.frames = frames;
            }
        }

        Some(sample)
    }

    fn append_sample(
        &mut self,
        sample: PerfSample,
        features: &PerfFeatureState,
        maps: &PerfRuntimeMaps,
        callchain_ids: &mut HashMap<(u32, Vec<u64>), u32>,
        next_callchain_id: &mut u32,
    ) {
        let timestamp = saturating_u64_to_i64(sample.time);
        self.start_ts = Some(
            self.start_ts
                .map_or(timestamp, |value| value.min(timestamp)),
        );
        self.end_ts = Some(self.end_ts.map_or(timestamp, |value| value.max(timestamp)));

        let frames = normalize_frames(&sample);
        let callchain_id = if frames.is_empty() {
            None
        } else {
            let key = (sample.pid, frames.clone());
            let id = if let Some(id) = callchain_ids.get(&key) {
                *id
            } else {
                let id = *next_callchain_id;
                *next_callchain_id = (*next_callchain_id).saturating_add(1);
                callchain_ids.insert(key, id);
                self.append_callchain_rows(id, &frames, sample.pid, sample.tid, features, maps);
                id
            };
            Some(id)
        };

        let event_type_id = features
            .event_config_by_id
            .get(&sample.id)
            .copied()
            .unwrap_or(0);
        let thread_state = features
            .event_names_by_index
            .get(event_type_id as usize)
            .map(|name| match name.as_str() {
                "sched:sched_waking" => "Running".to_string(),
                "sched:sched_cpu_off" => "Suspend".to_string(),
                _ => "-".to_string(),
            })
            .or_else(|| Some("-".to_string()));

        let id = self.tables.next_perf_sample_id();
        self.tables.push_perf_sample(PerfSampleRow {
            id,
            callchain_id,
            timestamp,
            thread_id: sample.tid,
            event_count: sample.period,
            event_type_id,
            timestamp_trace: timestamp,
            cpu_id: sample.cpu,
            thread_state,
        });
    }

    fn append_callchain_rows(
        &mut self,
        callchain_id: u32,
        frames: &[u64],
        pid: u32,
        tid: u32,
        features: &PerfFeatureState,
        maps: &PerfRuntimeMaps,
    ) {
        for (depth, ip) in frames.iter().rev().copied().enumerate() {
            let resolved = resolve_frame(features, maps, pid, tid, ip);
            let id = self.tables.next_perf_callchain_id();
            self.tables.push_perf_callchain(PerfCallchainRow {
                id,
                callchain_id,
                depth: depth as u32,
                ip,
                vaddr_in_file: resolved.vaddr_in_file,
                offset_to_vaddr: resolved.offset_to_vaddr,
                file_id: resolved.file_id,
                symbol_id: resolved.symbol_id,
                name: resolved.name,
                source_file_id: None,
                line_number: None,
            });
        }
    }

    fn push_report(&mut self, id: &mut u64, report_type: &str, report_value: String) {
        self.tables.push_perf_report(PerfReportRow {
            id: *id,
            report_type: report_type.to_string(),
            report_value: Some(report_value),
        });
        *id += 1;
    }

    fn finish(self) -> ParseResult<ParsedTrace> {
        let trace_id = format!("perf:{:016x}", self.input_hash);
        let tables = self
            .tables
            .finish(
                trace_id.clone(),
                self.start_ts,
                self.end_ts,
                "perf".to_string(),
            )
            .map_err(|err| {
                TraceEngineError::Engine(format!("failed to build Arrow tables: {err}"))
            })?;

        Ok(ParsedTrace {
            trace_id,
            start_ts: self.start_ts,
            end_ts: self.end_ts,
            clock_domain: "perf".to_string(),
            tables,
        })
    }
}

impl HarmonyTraceParser for PerfParser {
    fn parse_file(&mut self, path: &Path) -> ParseResult<ParsedTrace> {
        let bytes = fs::read(path)?;
        self.parse_bytes(&bytes)
    }

    fn parse_bytes(&mut self, bytes: &[u8]) -> ParseResult<ParsedTrace> {
        self.reset_for_input(bytes);
        self.parse_perf(bytes)?;
        let parser = std::mem::take(self);
        parser.finish()
    }
}

pub(crate) fn looks_like_perf(bytes: &[u8]) -> bool {
    bytes.starts_with(PERF_MAGIC)
}

fn parse_features(bytes: &[u8]) -> Vec<u32> {
    let mut features = Vec::new();
    for (byte_index, byte) in bytes.iter().copied().enumerate() {
        for bit in 0..8 {
            if byte & (1 << bit) != 0 {
                features.push((byte_index * 8 + bit) as u32);
            }
        }
    }
    features
}

fn recover_stack_frames(
    features: &PerfFeatureState,
    pid: u32,
    stack_id_value: u64,
) -> Option<Vec<u64>> {
    let table = features.stack_tables.get(&pid)?;
    let mut index = (stack_id_value & STACK_ID_INDEX_MASK) as u32;
    let mut remaining = stack_id_value >> STACK_ID_INDEX_BITS;
    if index == 0 || remaining == 0 {
        return Some(Vec::new());
    }

    let mut frames = Vec::new();
    while remaining > 0 {
        let node = *table.get(&index)?;
        let mut ip = node & NODE_IP_MASK;
        if node >> 63 != 0 {
            ip |= KERNEL_PREFIX;
        }
        frames.push(ip);
        let prev = ((node >> NODE_IP_BITS) & NODE_PREV_MASK) as u32;
        if prev == 0 {
            break;
        }
        index = prev;
        remaining -= 1;
    }
    Some(frames)
}

fn normalize_frames(sample: &PerfSample) -> Vec<u64> {
    let mut frames = if sample.frames.is_empty() {
        sample.ip.into_iter().collect::<Vec<_>>()
    } else {
        sample.frames.clone()
    };
    frames.retain(|ip| *ip < PERF_CONTEXT_MAX && *ip >= BAD_IP_ADDRESS);
    frames
}

fn parse_mmap_payload(
    record: &[u8],
    record_type: u32,
) -> Option<(u32, u32, u64, u64, u64, String)> {
    match record_type {
        PERF_RECORD_MMAP => {
            if record.len() < 40 {
                return None;
            }
            Some((
                read_u32_le(record, 8).ok()?,
                read_u32_le(record, 12).ok()?,
                read_u64_le(record, 16).ok()?,
                read_u64_le(record, 24).ok()?,
                read_u64_le(record, 32).ok()?,
                read_c_string(&record[40..]),
            ))
        }
        PERF_RECORD_MMAP2 => {
            if record.len() < 72 {
                return None;
            }
            Some((
                read_u32_le(record, 8).ok()?,
                read_u32_le(record, 12).ok()?,
                read_u64_le(record, 16).ok()?,
                read_u64_le(record, 24).ok()?,
                read_u64_le(record, 32).ok()?,
                read_c_string(&record[72..]),
            ))
        }
        _ => None,
    }
}

fn resolve_file_id(features: &PerfFeatureState, path: &str) -> Option<u64> {
    features.path_to_file_id.get(path).copied().or_else(|| {
        features
            .path_to_file_id
            .iter()
            .find(|(known_path, _)| {
                path.ends_with(known_path.as_str()) || known_path.ends_with(path)
            })
            .map(|(_, file_id)| *file_id)
    })
}

fn resolve_frame(
    features: &PerfFeatureState,
    maps: &PerfRuntimeMaps,
    pid: u32,
    tid: u32,
    ip: u64,
) -> ResolvedFrame {
    let Some(map) = find_map(maps, pid, tid, ip) else {
        return ResolvedFrame::default();
    };
    let file_id = map.file_id;
    let vaddr = ip.saturating_sub(map.start).saturating_add(map.pgoff);
    let mut resolved = ResolvedFrame {
        file_id,
        vaddr_in_file: Some(vaddr),
        ..ResolvedFrame::default()
    };

    if let Some(file_id) = file_id {
        if let Some(symbol) = find_symbol(features, file_id, vaddr) {
            resolved.symbol_id = Some(symbol.serial_id as u64);
            resolved.name = Some(symbol.name.clone());
            resolved.vaddr_in_file = Some(symbol.vaddr);
            resolved.offset_to_vaddr = Some(vaddr.saturating_sub(symbol.vaddr));
        }
    }
    resolved
}

fn find_map<'a>(maps: &'a PerfRuntimeMaps, pid: u32, tid: u32, ip: u64) -> Option<&'a PerfMap> {
    if let Some(map) = maps
        .by_tid
        .get(&tid)
        .and_then(|items| items.iter().rev().find(|map| map.contains(ip)))
    {
        return Some(map);
    }
    if let Some(map) = maps
        .by_pid
        .get(&pid)
        .and_then(|items| items.iter().rev().find(|map| map.contains(ip)))
    {
        return Some(map);
    }
    maps.kernel.iter().rev().find(|map| map.contains(ip))
}

fn find_symbol(features: &PerfFeatureState, file_id: u64, vaddr: u64) -> Option<&PerfSymbol> {
    features
        .symbols_by_file_id
        .get(&file_id)?
        .iter()
        .filter(|symbol| {
            vaddr >= symbol.vaddr
                && (symbol.len == 0 || vaddr < symbol.vaddr.saturating_add(symbol.len as u64))
        })
        .max_by_key(|symbol| symbol.vaddr)
        .or_else(|| {
            features
                .symbols_by_file_id
                .get(&file_id)?
                .iter()
                .filter(|symbol| vaddr >= symbol.vaddr)
                .max_by_key(|symbol| symbol.vaddr)
        })
}

fn read_perf_string_at(bytes: &[u8], offset: usize) -> Option<(String, usize)> {
    let len = read_u32_le(bytes, offset).ok()? as usize;
    if len == 0 {
        return None;
    }
    let start = offset.checked_add(4)?;
    let end = start.checked_add(len)?;
    let raw = bytes.get(start..end)?;
    let end_without_nul = raw
        .iter()
        .rposition(|byte| *byte != 0)
        .map(|index| index + 1)
        .unwrap_or(0);
    let text = String::from_utf8_lossy(&raw[..end_without_nul]).to_string();
    Some((text, end))
}

fn read_c_string(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).to_string()
}

fn empty_to_none(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn pop_u64(record: &[u8], offset: &mut usize, condition: bool) -> Option<Option<u64>> {
    if !condition {
        return Some(None);
    }
    let value = read_u64_le(record, *offset).ok()?;
    *offset += 8;
    Some(Some(value))
}

fn perf_record_name(record_type: u32) -> &'static str {
    match record_type {
        PERF_RECORD_MMAP => "mmap",
        PERF_RECORD_COMM => "comm",
        PERF_RECORD_SAMPLE => "sample",
        PERF_RECORD_MMAP2 => "mmap2",
        PERF_RECORD_AUXTRACE => "auxtrace",
        _ => "unknown",
    }
}

fn saturating_u64_to_i64(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

fn read_u16_le(bytes: &[u8], offset: usize) -> ParseResult<u16> {
    let end = offset + 2;
    let data = bytes
        .get(offset..end)
        .ok_or_else(|| TraceEngineError::Parse(format!("missing u16 at byte {offset}")))?;
    Ok(u16::from_le_bytes(
        data.try_into().expect("slice has length 2"),
    ))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> ParseResult<u32> {
    let end = offset + 4;
    let data = bytes
        .get(offset..end)
        .ok_or_else(|| TraceEngineError::Parse(format!("missing u32 at byte {offset}")))?;
    Ok(u32::from_le_bytes(
        data.try_into().expect("slice has length 4"),
    ))
}

fn read_u64_le(bytes: &[u8], offset: usize) -> ParseResult<u64> {
    let end = offset + 8;
    let data = bytes
        .get(offset..end)
        .ok_or_else(|| TraceEngineError::Parse(format!("missing u64 at byte {offset}")))?;
    Ok(u64::from_le_bytes(
        data.try_into().expect("slice has length 8"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parses_perf_header_and_records() {
        let mut bytes = vec![0u8; PERF_FILE_HEADER_MIN_SIZE];
        bytes[0..8].copy_from_slice(PERF_MAGIC);
        bytes[8..16].copy_from_slice(&(PERF_FILE_HEADER_MIN_SIZE as u64).to_le_bytes());
        bytes[16..24].copy_from_slice(&136u64.to_le_bytes());
        bytes[24..32].copy_from_slice(&(PERF_FILE_HEADER_MIN_SIZE as u64).to_le_bytes());
        bytes[32..40].copy_from_slice(&136u64.to_le_bytes());
        bytes[40..48].copy_from_slice(&240u64.to_le_bytes());
        bytes[48..56].copy_from_slice(&48u64.to_le_bytes());
        bytes.resize(240, 0);
        bytes[PERF_FILE_HEADER_MIN_SIZE + 24..PERF_FILE_HEADER_MIN_SIZE + 32]
            .copy_from_slice(&(PERF_SAMPLE_IP | PERF_SAMPLE_TID | PERF_SAMPLE_TIME).to_le_bytes());
        bytes.extend_from_slice(&PERF_RECORD_SAMPLE.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&32u16.to_le_bytes());
        bytes.extend_from_slice(&0x1234u64.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&100u64.to_le_bytes());
        bytes.extend_from_slice(&PERF_RECORD_COMM.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());

        let parsed = PerfParser::default()
            .parse_bytes(&bytes)
            .expect("parse perf");
        assert_eq!(parsed.tables.perf_sample.num_rows(), 1);
        assert_eq!(parsed.tables.perf_thread.num_rows(), 1);
        assert_eq!(parsed.tables.raw_event.num_rows(), 4);
    }

    #[test]
    fn detects_perf() {
        assert!(looks_like_perf(b"PERFILE2...."));
    }

    #[test]
    fn parses_repository_perf_fixture() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../tests/fixtures/traces/perfCompressed.data");
        if !fixture.exists() {
            eprintln!("skip missing fixture {}", fixture.display());
            return;
        }

        let parsed = PerfParser::default()
            .parse_file(&fixture)
            .expect("parse perf fixture");
        assert_eq!(parsed.tables.perf_sample.num_rows(), 378);
        assert_eq!(parsed.tables.perf_thread.num_rows(), 7);
        assert!(parsed.tables.perf_files.num_rows() > 0);
        assert!(parsed.tables.perf_callchain.num_rows() > 0);
    }
}
