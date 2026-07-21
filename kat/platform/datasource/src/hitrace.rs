use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs::{self, File},
    io::{self, BufReader, BufWriter, Read, Seek, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use arrow_array::{Int32Array, RecordBatch, StringArray, UInt32Array, UInt64Array};
use arrow_schema::{DataType, Field, Schema};
use prost::Message;

use crate::{
    DatasetWriteTarget,
    dataset_writer::{DatasetWriteError, DatasetWriter},
};

const HEADER_SIZE: usize = 1024;
const HEADER_MAGIC: u64 = 0x464F_5250_534F_484F;
const PROTOBUF_SECTION: u32 = 0;
const BATCH_ROWS: usize = 8192;
const TICKS_PER_SECOND: u64 = 1_000_000_000;

const HEADER_CLOCKS: [(&str, &str, usize); 6] = [
    ("boottime", "boottime", 60),
    ("realtime", "realtime", 68),
    ("realtime_coarse", "realtime_coarse", 76),
    ("monotonic", "monotonic", 84),
    ("monotonic_coarse", "monotonic_coarse", 92),
    ("monotonic_raw", "monotonic_raw", 100),
];

#[derive(Debug)]
pub struct ImportedHitrace {
    path: PathBuf,
    unsupported_plugins: Vec<String>,
    unsupported_section_types: Vec<u32>,
    unsupported_content: Vec<UnsupportedHitraceContent>,
}

impl ImportedHitrace {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn unsupported_plugins(&self) -> &[String] {
        &self.unsupported_plugins
    }

    pub fn unsupported_section_types(&self) -> &[u32] {
        &self.unsupported_section_types
    }

    pub fn unsupported_content(&self) -> &[UnsupportedHitraceContent] {
        &self.unsupported_content
    }
}

#[derive(Debug)]
pub struct UnsupportedHitraceContent {
    kind: &'static str,
    value: String,
    byte_offset: usize,
}

impl UnsupportedHitraceContent {
    pub fn kind(&self) -> &str {
        self.kind
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn byte_offset(&self) -> usize {
        self.byte_offset
    }
}

pub fn import_hitrace(
    trace: &Path,
    target: DatasetWriteTarget,
) -> Result<ImportedHitrace, HitraceImportError> {
    let source = canonical_source(trace)?;
    let file = File::open(&source).map_err(|source_error| HitraceImportError::ReadSource {
        path: source.clone(),
        source: source_error,
    })?;
    let mut decoded = decode_reader(BufReader::new(file))?;
    let switches = decoded
        .switches
        .take()
        .map(SwitchSpool::into_reader)
        .transpose()?;

    let mut writer = DatasetWriter::begin(target).map_err(HitraceImportError::Dataset)?;
    write_clock_domains(&mut writer, &decoded.clock_domains)?;
    write_clock_snapshots(&mut writer, &decoded.snapshots)?;
    if let Some(switches) = switches {
        write_sched_switches(&mut writer, switches, decoded.ftrace_clock.unwrap())?;
    }
    let path = writer.finish().map_err(HitraceImportError::Dataset)?;

    Ok(ImportedHitrace {
        path,
        unsupported_plugins: decoded.unsupported_plugins.into_iter().collect(),
        unsupported_section_types: decoded.unsupported_section_types.into_iter().collect(),
        unsupported_content: decoded.unsupported_content,
    })
}

fn canonical_source(path: &Path) -> Result<PathBuf, HitraceImportError> {
    let canonical =
        dunce::canonicalize(path).map_err(|source| HitraceImportError::CanonicalSource {
            path: path.to_path_buf(),
            source,
        })?;
    if canonical.to_str().is_none() {
        return Err(HitraceImportError::NonUnicodeSource { path: canonical });
    }
    let metadata =
        fs::metadata(&canonical).map_err(|source| HitraceImportError::InspectSource {
            path: canonical.clone(),
            source,
        })?;
    if !metadata.is_file() {
        return Err(HitraceImportError::SourceNotFile { path: canonical });
    }
    Ok(canonical)
}

struct DecodedTrace {
    switches: Option<SwitchSpool>,
    snapshots: Vec<SnapshotRow>,
    clock_domains: BTreeMap<String, String>,
    ftrace_clock: Option<FtraceClock>,
    unsupported_plugins: BTreeSet<String>,
    unsupported_section_types: BTreeSet<u32>,
    unsupported_content: Vec<UnsupportedHitraceContent>,
}

#[derive(Clone, Copy)]
enum FtraceClock {
    Boot,
    Mono,
    Global,
    Local,
}

impl FtraceClock {
    fn domain(self, cpu: u32) -> String {
        match self {
            Self::Boot => "boottime".to_owned(),
            Self::Mono => "monotonic".to_owned(),
            Self::Global => "ftrace_global".to_owned(),
            Self::Local => format!("ftrace_local_cpu_{cpu}"),
        }
    }

    fn clock_type(self) -> &'static str {
        match self {
            Self::Boot => "boottime",
            Self::Mono => "monotonic",
            Self::Global => "ftrace_global",
            Self::Local => "ftrace_local",
        }
    }
}

struct SwitchRow {
    clock_value: u64,
    cpu: u32,
    sequence: u64,
    previous_thread_id: i32,
    previous_thread_name: String,
    next_thread_id: i32,
    next_thread_name: String,
}

struct SwitchSpool {
    writer: BufWriter<File>,
    count: u64,
}

impl SwitchSpool {
    fn new() -> Result<Self, HitraceImportError> {
        let file = tempfile::tempfile().map_err(HitraceImportError::CreateSwitchSpool)?;
        Ok(Self {
            writer: BufWriter::new(file),
            count: 0,
        })
    }

    fn push(&mut self, row: &SwitchRow) -> Result<(), HitraceImportError> {
        write_spool_u64(&mut self.writer, row.clock_value)?;
        write_spool_u32(&mut self.writer, row.cpu)?;
        write_spool_u64(&mut self.writer, row.sequence)?;
        write_spool_i32(&mut self.writer, row.previous_thread_id)?;
        write_spool_string(&mut self.writer, &row.previous_thread_name)?;
        write_spool_i32(&mut self.writer, row.next_thread_id)?;
        write_spool_string(&mut self.writer, &row.next_thread_name)?;
        self.count = self
            .count
            .checked_add(1)
            .ok_or(HitraceImportError::SwitchCountOverflow)?;
        Ok(())
    }

    fn into_reader(mut self) -> Result<SwitchSpoolReader, HitraceImportError> {
        self.writer
            .flush()
            .map_err(HitraceImportError::WriteSwitchSpool)?;
        let mut file = self
            .writer
            .into_inner()
            .map_err(|error| HitraceImportError::WriteSwitchSpool(error.into_error()))?;
        file.rewind().map_err(HitraceImportError::ReadSwitchSpool)?;
        Ok(SwitchSpoolReader {
            reader: BufReader::new(file),
            remaining: self.count,
        })
    }
}

struct SwitchSpoolReader {
    reader: BufReader<File>,
    remaining: u64,
}

impl SwitchSpoolReader {
    fn read_batch(&mut self) -> Result<Vec<SwitchRow>, HitraceImportError> {
        let count = usize::try_from(self.remaining.min(BATCH_ROWS as u64)).unwrap();
        let mut rows = Vec::with_capacity(count);
        for _ in 0..count {
            rows.push(SwitchRow {
                clock_value: read_spool_u64(&mut self.reader)?,
                cpu: read_spool_u32(&mut self.reader)?,
                sequence: read_spool_u64(&mut self.reader)?,
                previous_thread_id: read_spool_i32(&mut self.reader)?,
                previous_thread_name: read_spool_string(&mut self.reader)?,
                next_thread_id: read_spool_i32(&mut self.reader)?,
                next_thread_name: read_spool_string(&mut self.reader)?,
            });
        }
        self.remaining -= count as u64;
        Ok(rows)
    }
}

struct SnapshotRow {
    snapshot_id: u64,
    clock_domain: String,
    clock_value: u64,
}

#[cfg(test)]
fn decode(bytes: &[u8]) -> Result<DecodedTrace, HitraceImportError> {
    decode_reader(std::io::Cursor::new(bytes))
}

fn decode_reader(mut reader: impl Read) -> Result<DecodedTrace, HitraceImportError> {
    let mut offset = 0_usize;
    let mut header = read_section_header(&mut reader, offset)?.ok_or_else(|| {
        HitraceImportError::InvalidContainer {
            detail: "file is shorter than the 1024-byte Hitrace header".to_owned(),
        }
    })?;
    let mut decoded = DecodedTrace {
        switches: None,
        snapshots: HEADER_CLOCKS
            .iter()
            .map(|(domain, _, clock_offset)| SnapshotRow {
                snapshot_id: 0,
                clock_domain: (*domain).to_owned(),
                clock_value: read_u64(&header, *clock_offset).expect("header length checked"),
            })
            .collect(),
        clock_domains: HEADER_CLOCKS
            .iter()
            .map(|(domain, clock_type, _)| ((*domain).to_owned(), (*clock_type).to_owned()))
            .collect(),
        ftrace_clock: None,
        unsupported_plugins: BTreeSet::new(),
        unsupported_section_types: BTreeSet::new(),
        unsupported_content: Vec::new(),
    };
    let mut reported_clocks = BTreeSet::new();
    let mut detail_cpus = BTreeSet::new();
    let mut end_stats: Option<HashMap<u32, PerCpuStatsMsg>> = None;
    let mut next_snapshot_id = 1_u64;
    let mut last_switch: HashMap<u32, (u64, i32, u64)> = HashMap::new();
    loop {
        let section = section_header(&header, offset)?;
        let body_length = section.length - HEADER_SIZE;
        if section.data_type != PROTOBUF_SECTION {
            decoded.unsupported_section_types.insert(section.data_type);
            decoded.unsupported_content.push(UnsupportedHitraceContent {
                kind: "section_type",
                value: section.data_type.to_string(),
                byte_offset: offset,
            });
            discard_exact(&mut reader, body_length, offset + HEADER_SIZE)?;
        } else {
            decode_frames(
                &mut reader,
                body_length,
                offset + HEADER_SIZE,
                &mut decoded,
                &mut reported_clocks,
                &mut detail_cpus,
                &mut end_stats,
                &mut next_snapshot_id,
                &mut last_switch,
            )?;
        }
        offset = offset.checked_add(section.length).ok_or_else(|| {
            HitraceImportError::InvalidContainer {
                detail: format!("section length overflows at byte {offset}"),
            }
        })?;
        let Some(next_header) = read_section_header(&mut reader, offset)? else {
            break;
        };
        header = next_header;
    }

    if decoded.switches.is_some() {
        let clock = match reported_clocks.len() {
            0 => return Err(HitraceImportError::MissingTraceClock),
            1 => *reported_clocks.iter().next().unwrap(),
            _ => {
                return Err(HitraceImportError::ConflictingTraceClocks {
                    clocks: reported_clocks.into_iter().map(str::to_owned).collect(),
                });
            }
        };
        let clock = parse_ftrace_clock(clock)?;
        let stats = end_stats.ok_or(HitraceImportError::MissingEndStats)?;
        for cpu in &detail_cpus {
            if !stats.contains_key(cpu) {
                return Err(HitraceImportError::IncompleteEndStats { cpu: *cpu });
            }
        }
        for cpu in last_switch.keys() {
            let domain = clock.domain(*cpu);
            decoded
                .clock_domains
                .entry(domain)
                .or_insert_with(|| clock.clock_type().to_owned());
        }
        decoded.ftrace_clock = Some(clock);
    } else if reported_clocks.len() > 1 {
        return Err(HitraceImportError::ConflictingTraceClocks {
            clocks: reported_clocks.into_iter().map(str::to_owned).collect(),
        });
    } else if let Some(clock) = reported_clocks.iter().next() {
        parse_ftrace_clock(clock)?;
    }

    Ok(decoded)
}

struct SectionHeader {
    length: usize,
    data_type: u32,
}

fn section_header(
    header: &[u8; HEADER_SIZE],
    offset: usize,
) -> Result<SectionHeader, HitraceImportError> {
    let magic = read_u64(header, 0).unwrap();
    if magic != HEADER_MAGIC {
        return Err(HitraceImportError::InvalidContainer {
            detail: format!("invalid Hitrace section magic at byte {offset}"),
        });
    }
    let length = usize::try_from(read_u64(header, 8).unwrap()).map_err(|_| {
        HitraceImportError::InvalidContainer {
            detail: format!("section length is not representable at byte {offset}"),
        }
    })?;
    if length < HEADER_SIZE {
        return Err(HitraceImportError::InvalidContainer {
            detail: format!("invalid section length {length} at byte {offset}"),
        });
    }
    Ok(SectionHeader {
        length,
        data_type: read_u32(header, 56).unwrap(),
    })
}

fn read_section_header(
    reader: &mut impl Read,
    offset: usize,
) -> Result<Option<[u8; HEADER_SIZE]>, HitraceImportError> {
    let mut header = [0; HEADER_SIZE];
    let first = reader
        .read(&mut header[..1])
        .map_err(HitraceImportError::ReadContainer)?;
    if first == 0 {
        return Ok(None);
    }
    read_exact_container(reader, &mut header[1..], offset, "Hitrace section header")?;
    Ok(Some(header))
}

fn discard_exact(
    reader: &mut impl Read,
    mut remaining: usize,
    offset: usize,
) -> Result<(), HitraceImportError> {
    let mut buffer = [0; 8192];
    while remaining > 0 {
        let count = remaining.min(buffer.len());
        read_exact_container(reader, &mut buffer[..count], offset, "section body")?;
        remaining -= count;
    }
    Ok(())
}

fn read_exact_container(
    reader: &mut impl Read,
    buffer: &mut [u8],
    offset: usize,
    context: &str,
) -> Result<(), HitraceImportError> {
    reader.read_exact(buffer).map_err(|source| {
        if source.kind() == io::ErrorKind::UnexpectedEof {
            HitraceImportError::InvalidContainer {
                detail: format!("truncated {context} at byte {offset}"),
            }
        } else {
            HitraceImportError::ReadContainer(source)
        }
    })
}

#[allow(clippy::too_many_arguments)]
fn decode_frames(
    reader: &mut impl Read,
    body_length: usize,
    body_offset: usize,
    decoded: &mut DecodedTrace,
    reported_clocks: &mut BTreeSet<&'static str>,
    detail_cpus: &mut BTreeSet<u32>,
    end_stats: &mut Option<HashMap<u32, PerCpuStatsMsg>>,
    next_snapshot_id: &mut u64,
    last_switch: &mut HashMap<u32, (u64, i32, u64)>,
) -> Result<(), HitraceImportError> {
    let mut offset = 0;
    while offset < body_length {
        if body_length.saturating_sub(offset) < 4 {
            return Err(HitraceImportError::InvalidContainer {
                detail: format!(
                    "truncated protobuf frame length at byte {}",
                    body_offset + offset
                ),
            });
        }
        let mut length_bytes = [0; 4];
        read_exact_container(
            reader,
            &mut length_bytes,
            body_offset + offset,
            "protobuf frame length",
        )?;
        let length = u32::from_le_bytes(length_bytes) as usize;
        let frame_start = offset + 4;
        let frame_end = frame_start.checked_add(length).ok_or_else(|| {
            HitraceImportError::InvalidContainer {
                detail: format!(
                    "protobuf frame length overflows at byte {}",
                    body_offset + offset
                ),
            }
        })?;
        if frame_end > body_length {
            return Err(HitraceImportError::InvalidContainer {
                detail: format!("truncated protobuf frame at byte {}", body_offset + offset),
            });
        }
        let frame = read_frame(reader, length, body_offset + frame_start)?;
        let envelope = ProfilerPluginData::decode(frame.as_slice()).map_err(|source| {
            HitraceImportError::InvalidEnvelope {
                byte_offset: body_offset + frame_start,
                source,
            }
        })?;
        let (plugin, is_config) = match envelope.name.strip_suffix("_config") {
            Some(plugin) => (plugin, true),
            None => (envelope.name.as_str(), false),
        };
        match (plugin, is_config) {
            ("ftrace-plugin", false) => decode_ftrace_result(
                &envelope.data,
                body_offset + frame_start,
                decoded,
                reported_clocks,
                detail_cpus,
                end_stats,
                next_snapshot_id,
                last_switch,
            )?,
            ("ftrace-plugin", true) => {}
            (plugin, _) => {
                decoded.unsupported_plugins.insert(plugin.to_owned());
                decoded.unsupported_content.push(UnsupportedHitraceContent {
                    kind: "plugin",
                    value: plugin.to_owned(),
                    byte_offset: body_offset + frame_start,
                });
            }
        }
        offset = frame_end;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn decode_ftrace_result(
    bytes: &[u8],
    byte_offset: usize,
    decoded: &mut DecodedTrace,
    reported_clocks: &mut BTreeSet<&'static str>,
    detail_cpus: &mut BTreeSet<u32>,
    end_stats: &mut Option<HashMap<u32, PerCpuStatsMsg>>,
    next_snapshot_id: &mut u64,
    last_switch: &mut HashMap<u32, (u64, i32, u64)>,
) -> Result<(), HitraceImportError> {
    let result = TracePluginResult::decode(bytes).map_err(|source| {
        HitraceImportError::InvalidFtracePayload {
            byte_offset,
            source,
        }
    })?;

    for stats in result.ftrace_cpu_stats {
        if !matches!(stats.status, 0 | 1) {
            return Err(HitraceImportError::InvalidStatsStatus {
                status: stats.status,
            });
        }
        if !stats.trace_clock.trim().is_empty() {
            let normalized = normalize_trace_clock(&stats.trace_clock)?;
            reported_clocks.insert(normalized);
        }
        if stats.status == 1 {
            if end_stats.is_some() {
                return Err(HitraceImportError::DuplicateEndStats);
            }
            let mut snapshot = HashMap::new();
            for per_cpu in stats.per_cpu_stats {
                let cpu = u32::try_from(per_cpu.cpu)
                    .map_err(|_| HitraceImportError::CpuOutOfRange { cpu: per_cpu.cpu })?;
                if snapshot.insert(cpu, per_cpu.clone()).is_some() {
                    return Err(HitraceImportError::DuplicateEndCpu { cpu });
                }
                if per_cpu.overrun != 0
                    || per_cpu.commit_overrun != 0
                    || per_cpu.dropped_events != 0
                {
                    return Err(HitraceImportError::LostEvents {
                        cpu,
                        overrun: per_cpu.overrun,
                        commit_overrun: per_cpu.commit_overrun,
                        dropped_events: per_cpu.dropped_events,
                    });
                }
            }
            *end_stats = Some(snapshot);
        }
    }

    if !result.clocks_detail.is_empty() {
        let snapshot_id = *next_snapshot_id;
        *next_snapshot_id = next_snapshot_id
            .checked_add(1)
            .ok_or(HitraceImportError::SnapshotIdOverflow)?;
        let mut domains = HashSet::new();
        for clock in result.clocks_detail {
            let domain = snapshot_clock_domain(clock.id)?;
            if !domains.insert(domain) {
                return Err(HitraceImportError::DuplicateSnapshotClock {
                    snapshot_id,
                    clock_domain: domain.to_owned(),
                });
            }
            let time = clock
                .time
                .ok_or_else(|| HitraceImportError::MissingSnapshotTime {
                    snapshot_id,
                    clock_domain: domain.to_owned(),
                })?;
            if time.tv_nsec >= TICKS_PER_SECOND as u32 {
                return Err(HitraceImportError::InvalidNanoseconds {
                    snapshot_id,
                    clock_domain: domain.to_owned(),
                    nanoseconds: time.tv_nsec,
                });
            }
            let value = u64::from(time.tv_sec)
                .checked_mul(TICKS_PER_SECOND)
                .and_then(|value| value.checked_add(u64::from(time.tv_nsec)))
                .ok_or_else(|| HitraceImportError::SnapshotClockOverflow {
                    snapshot_id,
                    clock_domain: domain.to_owned(),
                })?;
            decoded.snapshots.push(SnapshotRow {
                snapshot_id,
                clock_domain: domain.to_owned(),
                clock_value: value,
            });
        }
    }

    for detail in result.ftrace_cpu_detail {
        detail_cpus.insert(detail.cpu);
        if detail.overwrite != 0 {
            return Err(HitraceImportError::PageOverwrite {
                cpu: detail.cpu,
                overwrite: detail.overwrite,
            });
        }
        for event in detail.event {
            let Some(switch) = event.sched_switch_format else {
                continue;
            };
            let sequence = match last_switch.get(&detail.cpu) {
                None => 0,
                Some((previous_clock, previous_next_thread, previous_sequence)) => {
                    if event.timestamp < *previous_clock {
                        return Err(HitraceImportError::ClockWentBackwards {
                            cpu: detail.cpu,
                            previous: *previous_clock,
                            current: event.timestamp,
                        });
                    }
                    if switch.prev_pid != *previous_next_thread {
                        return Err(HitraceImportError::BrokenThreadContinuity {
                            cpu: detail.cpu,
                            expected_previous_thread_id: *previous_next_thread,
                            actual_previous_thread_id: switch.prev_pid,
                        });
                    }
                    previous_sequence
                        .checked_add(1)
                        .ok_or(HitraceImportError::SequenceOverflow { cpu: detail.cpu })?
                }
            };
            last_switch.insert(detail.cpu, (event.timestamp, switch.next_pid, sequence));
            if decoded.switches.is_none() {
                decoded.switches = Some(SwitchSpool::new()?);
            }
            decoded.switches.as_mut().unwrap().push(&SwitchRow {
                clock_value: event.timestamp,
                cpu: detail.cpu,
                sequence,
                previous_thread_id: switch.prev_pid,
                previous_thread_name: switch.prev_comm,
                next_thread_id: switch.next_pid,
                next_thread_name: switch.next_comm,
            })?;
        }
    }
    Ok(())
}

fn normalize_trace_clock(value: &str) -> Result<&'static str, HitraceImportError> {
    match value.trim() {
        "boot" => Ok("boot"),
        "mono" => Ok("mono"),
        "global" => Ok("global"),
        "local" => Ok("local"),
        value => Err(HitraceImportError::UnsupportedTraceClock {
            clock: value.to_owned(),
        }),
    }
}

fn parse_ftrace_clock(value: &str) -> Result<FtraceClock, HitraceImportError> {
    match value {
        "boot" => Ok(FtraceClock::Boot),
        "mono" => Ok(FtraceClock::Mono),
        "global" => Ok(FtraceClock::Global),
        "local" => Ok(FtraceClock::Local),
        _ => Err(HitraceImportError::UnsupportedTraceClock {
            clock: value.to_owned(),
        }),
    }
}

fn snapshot_clock_domain(id: i32) -> Result<&'static str, HitraceImportError> {
    match id {
        1 => Ok("boottime"),
        2 => Ok("realtime"),
        3 => Ok("realtime_coarse"),
        4 => Ok("monotonic"),
        5 => Ok("monotonic_coarse"),
        6 => Ok("monotonic_raw"),
        _ => Err(HitraceImportError::UnsupportedSnapshotClock { id }),
    }
}

fn write_clock_domains(
    writer: &mut DatasetWriter,
    domains: &BTreeMap<String, String>,
) -> Result<(), HitraceImportError> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("clock_domain", DataType::Utf8, false),
        Field::new("clock_type", DataType::Utf8, false),
        Field::new("ticks_per_second", DataType::UInt64, false),
    ]));
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(StringArray::from_iter_values(domains.keys())),
            Arc::new(StringArray::from_iter_values(domains.values())),
            Arc::new(UInt64Array::from(vec![TICKS_PER_SECOND; domains.len()])),
        ],
    )
    .map_err(HitraceImportError::Arrow)?;
    let mut table = writer
        .begin_table("clock_domain", schema)
        .map_err(HitraceImportError::Dataset)?;
    table.write(&batch).map_err(HitraceImportError::Dataset)?;
    table.finish().map_err(HitraceImportError::Dataset)
}

fn write_clock_snapshots(
    writer: &mut DatasetWriter,
    snapshots: &[SnapshotRow],
) -> Result<(), HitraceImportError> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("snapshot_id", DataType::UInt64, false),
        Field::new("clock_domain", DataType::Utf8, false),
        Field::new("clock_value", DataType::UInt64, false),
    ]));
    let mut table = writer
        .begin_table("clock_snapshot", schema.clone())
        .map_err(HitraceImportError::Dataset)?;
    for rows in snapshots.chunks(BATCH_ROWS) {
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(UInt64Array::from_iter_values(
                    rows.iter().map(|row| row.snapshot_id),
                )),
                Arc::new(StringArray::from_iter_values(
                    rows.iter().map(|row| row.clock_domain.as_str()),
                )),
                Arc::new(UInt64Array::from_iter_values(
                    rows.iter().map(|row| row.clock_value),
                )),
            ],
        )
        .map_err(HitraceImportError::Arrow)?;
        table.write(&batch).map_err(HitraceImportError::Dataset)?;
    }
    table.finish().map_err(HitraceImportError::Dataset)
}

fn write_sched_switches(
    writer: &mut DatasetWriter,
    mut switches: SwitchSpoolReader,
    clock: FtraceClock,
) -> Result<(), HitraceImportError> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("clock_domain", DataType::Utf8, false),
        Field::new("clock_value", DataType::UInt64, false),
        Field::new("cpu", DataType::UInt32, false),
        Field::new("cpu_switch_sequence", DataType::UInt64, false),
        Field::new("previous_thread_id", DataType::Int32, false),
        Field::new("previous_thread_name", DataType::Utf8, false),
        Field::new("next_thread_id", DataType::Int32, false),
        Field::new("next_thread_name", DataType::Utf8, false),
    ]));
    let mut table = writer
        .begin_table("sched_switch", schema.clone())
        .map_err(HitraceImportError::Dataset)?;
    while switches.remaining > 0 {
        let rows = switches.read_batch()?;
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(StringArray::from_iter_values(
                    rows.iter().map(|row| clock.domain(row.cpu)),
                )),
                Arc::new(UInt64Array::from_iter_values(
                    rows.iter().map(|row| row.clock_value),
                )),
                Arc::new(UInt32Array::from_iter_values(
                    rows.iter().map(|row| row.cpu),
                )),
                Arc::new(UInt64Array::from_iter_values(
                    rows.iter().map(|row| row.sequence),
                )),
                Arc::new(Int32Array::from_iter_values(
                    rows.iter().map(|row| row.previous_thread_id),
                )),
                Arc::new(StringArray::from_iter_values(
                    rows.iter().map(|row| row.previous_thread_name.as_str()),
                )),
                Arc::new(Int32Array::from_iter_values(
                    rows.iter().map(|row| row.next_thread_id),
                )),
                Arc::new(StringArray::from_iter_values(
                    rows.iter().map(|row| row.next_thread_name.as_str()),
                )),
            ],
        )
        .map_err(HitraceImportError::Arrow)?;
        table.write(&batch).map_err(HitraceImportError::Dataset)?;
    }
    table.finish().map_err(HitraceImportError::Dataset)
}

fn read_frame(
    reader: &mut impl Read,
    length: usize,
    offset: usize,
) -> Result<Vec<u8>, HitraceImportError> {
    let mut frame = Vec::new();
    let mut remaining = length;
    let mut buffer = [0; 8192];
    while remaining > 0 {
        let count = remaining.min(buffer.len());
        let read = reader
            .read(&mut buffer[..count])
            .map_err(HitraceImportError::ReadContainer)?;
        if read == 0 {
            return Err(HitraceImportError::InvalidContainer {
                detail: format!("truncated protobuf frame at byte {offset}"),
            });
        }
        frame.extend_from_slice(&buffer[..read]);
        remaining -= read;
    }
    Ok(frame)
}

fn write_spool_u32(writer: &mut impl Write, value: u32) -> Result<(), HitraceImportError> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(HitraceImportError::WriteSwitchSpool)
}

fn write_spool_u64(writer: &mut impl Write, value: u64) -> Result<(), HitraceImportError> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(HitraceImportError::WriteSwitchSpool)
}

fn write_spool_i32(writer: &mut impl Write, value: i32) -> Result<(), HitraceImportError> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(HitraceImportError::WriteSwitchSpool)
}

fn write_spool_string(writer: &mut impl Write, value: &str) -> Result<(), HitraceImportError> {
    let length = u32::try_from(value.len()).map_err(|_| HitraceImportError::SwitchNameTooLong)?;
    write_spool_u32(writer, length)?;
    writer
        .write_all(value.as_bytes())
        .map_err(HitraceImportError::WriteSwitchSpool)
}

fn read_spool_u32(reader: &mut impl Read) -> Result<u32, HitraceImportError> {
    let mut bytes = [0; 4];
    reader
        .read_exact(&mut bytes)
        .map_err(HitraceImportError::ReadSwitchSpool)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_spool_u64(reader: &mut impl Read) -> Result<u64, HitraceImportError> {
    let mut bytes = [0; 8];
    reader
        .read_exact(&mut bytes)
        .map_err(HitraceImportError::ReadSwitchSpool)?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_spool_i32(reader: &mut impl Read) -> Result<i32, HitraceImportError> {
    let mut bytes = [0; 4];
    reader
        .read_exact(&mut bytes)
        .map_err(HitraceImportError::ReadSwitchSpool)?;
    Ok(i32::from_le_bytes(bytes))
}

fn read_spool_string(reader: &mut impl Read) -> Result<String, HitraceImportError> {
    let length = read_spool_u32(reader)? as usize;
    let mut bytes = vec![0; length];
    reader
        .read_exact(&mut bytes)
        .map_err(HitraceImportError::ReadSwitchSpool)?;
    String::from_utf8(bytes).map_err(HitraceImportError::InvalidSwitchSpoolString)
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    bytes
        .get(offset..offset + 4)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
}

fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    bytes
        .get(offset..offset + 8)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u64::from_le_bytes)
}

#[derive(Clone, PartialEq, Message)]
struct ProfilerPluginData {
    #[prost(string, tag = "1")]
    name: String,
    #[prost(bytes = "vec", tag = "3")]
    data: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
struct TracePluginResult {
    #[prost(message, repeated, tag = "1")]
    ftrace_cpu_stats: Vec<FtraceCpuStatsMsg>,
    #[prost(message, repeated, tag = "2")]
    ftrace_cpu_detail: Vec<FtraceCpuDetailMsg>,
    #[prost(message, repeated, tag = "6")]
    clocks_detail: Vec<ClockDetailMsg>,
}

#[derive(Clone, PartialEq, Message)]
struct ClockDetailMsg {
    #[prost(enumeration = "ClockId", tag = "1")]
    id: i32,
    #[prost(message, optional, tag = "2")]
    time: Option<TimeSpec>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
enum ClockId {
    Unknown = 0,
    Boottime = 1,
    Realtime = 2,
    RealtimeCoarse = 3,
    Monotonic = 4,
    MonotonicCoarse = 5,
    MonotonicRaw = 6,
}

#[derive(Clone, PartialEq, Message)]
struct TimeSpec {
    #[prost(uint32, tag = "1")]
    tv_sec: u32,
    #[prost(uint32, tag = "2")]
    tv_nsec: u32,
}

#[derive(Clone, PartialEq, Message)]
struct FtraceCpuStatsMsg {
    #[prost(enumeration = "StatsStatus", tag = "1")]
    status: i32,
    #[prost(message, repeated, tag = "2")]
    per_cpu_stats: Vec<PerCpuStatsMsg>,
    #[prost(string, tag = "3")]
    trace_clock: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
enum StatsStatus {
    TraceStart = 0,
    TraceEnd = 1,
}

#[derive(Clone, PartialEq, Message)]
struct PerCpuStatsMsg {
    #[prost(uint64, tag = "1")]
    cpu: u64,
    #[prost(uint64, tag = "3")]
    overrun: u64,
    #[prost(uint64, tag = "4")]
    commit_overrun: u64,
    #[prost(uint64, tag = "8")]
    dropped_events: u64,
}

#[derive(Clone, PartialEq, Message)]
struct FtraceCpuDetailMsg {
    #[prost(uint32, tag = "1")]
    cpu: u32,
    #[prost(message, repeated, tag = "2")]
    event: Vec<FtraceEvent>,
    #[prost(uint64, tag = "3")]
    overwrite: u64,
}

#[derive(Clone, PartialEq, Message)]
struct FtraceEvent {
    #[prost(uint64, tag = "1")]
    timestamp: u64,
    #[prost(message, optional, tag = "2417")]
    sched_switch_format: Option<SchedSwitchFormat>,
}

#[derive(Clone, PartialEq, Message)]
struct SchedSwitchFormat {
    #[prost(string, tag = "1")]
    prev_comm: String,
    #[prost(int32, tag = "2")]
    prev_pid: i32,
    #[prost(string, tag = "5")]
    next_comm: String,
    #[prost(int32, tag = "6")]
    next_pid: i32,
}

#[derive(Debug, thiserror::Error)]
pub enum HitraceImportError {
    #[error("failed to resolve Hitrace source {path}")]
    CanonicalSource {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Hitrace source path cannot be represented as native Unicode: {path:?}")]
    NonUnicodeSource { path: PathBuf },
    #[error("failed to inspect Hitrace source {path}")]
    InspectSource {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Hitrace source is not an ordinary file: {path}")]
    SourceNotFile { path: PathBuf },
    #[error("failed to read Hitrace source {path}")]
    ReadSource {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid Hitrace container: {detail}")]
    InvalidContainer { detail: String },
    #[error("failed while reading Hitrace container")]
    ReadContainer(#[source] io::Error),
    #[error("invalid profiler envelope at byte {byte_offset}")]
    InvalidEnvelope {
        byte_offset: usize,
        #[source]
        source: prost::DecodeError,
    },
    #[error("invalid ftrace-plugin payload at byte {byte_offset}")]
    InvalidFtracePayload {
        byte_offset: usize,
        #[source]
        source: prost::DecodeError,
    },
    #[error("unsupported ftrace trace_clock {clock:?}")]
    UnsupportedTraceClock { clock: String },
    #[error("sched_switch events have no FtraceCpuStatsMsg.trace_clock evidence")]
    MissingTraceClock,
    #[error("ftrace reports conflicting trace clocks: {clocks:?}")]
    ConflictingTraceClocks { clocks: Vec<String> },
    #[error("ftrace capture has more than one TRACE_END snapshot")]
    DuplicateEndStats,
    #[error("invalid FtraceCpuStatsMsg status {status}")]
    InvalidStatsStatus { status: i32 },
    #[error("ftrace TRACE_END snapshot repeats CPU {cpu}")]
    DuplicateEndCpu { cpu: u32 },
    #[error("ftrace capture has sched_switch events but no TRACE_END snapshot")]
    MissingEndStats,
    #[error("ftrace TRACE_END snapshot does not cover event CPU {cpu}")]
    IncompleteEndStats { cpu: u32 },
    #[error("ftrace CPU id {cpu} cannot be represented as UInt32")]
    CpuOutOfRange { cpu: u64 },
    #[error(
        "ftrace capture lost events on CPU {cpu}: overrun={overrun}, commit_overrun={commit_overrun}, dropped_events={dropped_events}"
    )]
    LostEvents {
        cpu: u32,
        overrun: u64,
        commit_overrun: u64,
        dropped_events: u64,
    },
    #[error("ftrace page overwrite is nonzero on CPU {cpu}: {overwrite}")]
    PageOverwrite { cpu: u32, overwrite: u64 },
    #[error("sched_switch clock went backwards on CPU {cpu}: {previous} then {current}")]
    ClockWentBackwards {
        cpu: u32,
        previous: u64,
        current: u64,
    },
    #[error(
        "sched_switch thread continuity is broken on CPU {cpu}: expected previous_thread_id {expected_previous_thread_id}, got {actual_previous_thread_id}"
    )]
    BrokenThreadContinuity {
        cpu: u32,
        expected_previous_thread_id: i32,
        actual_previous_thread_id: i32,
    },
    #[error("sched_switch sequence overflow on CPU {cpu}")]
    SequenceOverflow { cpu: u32 },
    #[error("sched_switch count overflow")]
    SwitchCountOverflow,
    #[error("sched_switch thread name is too long to spool")]
    SwitchNameTooLong,
    #[error("failed to create the bounded sched_switch preflight spool")]
    CreateSwitchSpool(#[source] io::Error),
    #[error("failed to write the bounded sched_switch preflight spool")]
    WriteSwitchSpool(#[source] io::Error),
    #[error("failed to read the bounded sched_switch preflight spool")]
    ReadSwitchSpool(#[source] io::Error),
    #[error("bounded sched_switch preflight spool contains invalid UTF-8")]
    InvalidSwitchSpoolString(#[source] std::string::FromUtf8Error),
    #[error("clock snapshot id overflow")]
    SnapshotIdOverflow,
    #[error("unsupported clock snapshot id {id}")]
    UnsupportedSnapshotClock { id: i32 },
    #[error("clock snapshot {snapshot_id} repeats domain {clock_domain:?}")]
    DuplicateSnapshotClock {
        snapshot_id: u64,
        clock_domain: String,
    },
    #[error("clock snapshot {snapshot_id} has no time for domain {clock_domain:?}")]
    MissingSnapshotTime {
        snapshot_id: u64,
        clock_domain: String,
    },
    #[error(
        "clock snapshot {snapshot_id} has invalid nanoseconds {nanoseconds} for domain {clock_domain:?}"
    )]
    InvalidNanoseconds {
        snapshot_id: u64,
        clock_domain: String,
        nanoseconds: u32,
    },
    #[error("clock snapshot {snapshot_id} overflows UInt64 for domain {clock_domain:?}")]
    SnapshotClockOverflow {
        snapshot_id: u64,
        clock_domain: String,
    },
    #[error("failed to create Arrow batch for Hitrace facts")]
    Arrow(#[source] arrow_schema::ArrowError),
    #[error("failed to publish Hitrace Dataset")]
    Dataset(#[source] DatasetWriteError),
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use arrow_array::{Array, StringArray, UInt32Array, UInt64Array};
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use tempfile::tempdir;

    use super::*;

    fn switch(timestamp: u64, previous: i32, next: i32) -> FtraceEvent {
        FtraceEvent {
            timestamp,
            sched_switch_format: Some(SchedSwitchFormat {
                prev_comm: format!("thread-{previous}"),
                prev_pid: previous,
                next_comm: format!("thread-{next}"),
                next_pid: next,
            }),
        }
    }

    fn stats(status: StatsStatus, clock: &str, cpus: &[u32]) -> FtraceCpuStatsMsg {
        FtraceCpuStatsMsg {
            status: status as i32,
            trace_clock: clock.to_owned(),
            per_cpu_stats: cpus
                .iter()
                .map(|cpu| PerCpuStatsMsg {
                    cpu: u64::from(*cpu),
                    ..Default::default()
                })
                .collect(),
        }
    }

    fn result(
        clock: &str,
        details: Vec<FtraceCpuDetailMsg>,
        clocks_detail: Vec<ClockDetailMsg>,
    ) -> TracePluginResult {
        let cpus = details
            .iter()
            .map(|detail| detail.cpu)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        TracePluginResult {
            ftrace_cpu_stats: vec![
                stats(StatsStatus::TraceStart, clock, &cpus),
                stats(StatsStatus::TraceEnd, clock, &cpus),
            ],
            ftrace_cpu_detail: details,
            clocks_detail,
        }
    }

    fn envelope(name: &str, data: Vec<u8>) -> Vec<u8> {
        ProfilerPluginData {
            name: name.to_owned(),
            data,
        }
        .encode_to_vec()
    }

    fn section(data_type: u32, frames: &[Vec<u8>]) -> Vec<u8> {
        let mut body = Vec::new();
        for frame in frames {
            body.extend_from_slice(&(frame.len() as u32).to_le_bytes());
            body.extend_from_slice(frame);
        }
        let mut section = vec![0; HEADER_SIZE];
        section[0..8].copy_from_slice(&HEADER_MAGIC.to_le_bytes());
        section[8..16].copy_from_slice(&((HEADER_SIZE + body.len()) as u64).to_le_bytes());
        section[56..60].copy_from_slice(&data_type.to_le_bytes());
        for (index, (_, _, offset)) in HEADER_CLOCKS.iter().enumerate() {
            section[*offset..*offset + 8].copy_from_slice(&(100 + index as u64).to_le_bytes());
        }
        section.extend_from_slice(&body);
        section
    }

    fn fixture(result: TracePluginResult) -> Vec<u8> {
        section(
            PROTOBUF_SECTION,
            &[envelope("ftrace-plugin", result.encode_to_vec())],
        )
    }

    fn detail(cpu: u32, event: Vec<FtraceEvent>) -> FtraceCpuDetailMsg {
        FtraceCpuDetailMsg {
            cpu,
            event,
            overwrite: 0,
        }
    }

    fn import(bytes: &[u8]) -> (tempfile::TempDir, ImportedHitrace) {
        let temp = tempdir().unwrap();
        let source = temp.path().join("capture.htrace");
        fs::write(&source, bytes).unwrap();
        let imported = import_hitrace(
            &source,
            DatasetWriteTarget::write_to_empty(temp.path().join("dataset")),
        )
        .unwrap();
        (temp, imported)
    }

    fn failure(result: TracePluginResult) -> HitraceImportError {
        decode(&fixture(result)).err().expect("expected failure")
    }

    fn batches(path: &Path) -> Vec<RecordBatch> {
        ParquetRecordBatchReaderBuilder::try_new(File::open(path).unwrap())
            .unwrap()
            .build()
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

    #[test]
    fn imports_multi_cpu_source_order_same_clock_and_snapshot_evidence() {
        let clocks = vec![ClockDetailMsg {
            id: ClockId::Boottime as i32,
            time: Some(TimeSpec {
                tv_sec: 2,
                tv_nsec: 3,
            }),
        }];
        let trace = result(
            "boot",
            vec![
                detail(1, vec![switch(10, 0, 11), switch(10, 11, 12)]),
                detail(0, vec![switch(7, 0, 21), switch(9, 21, 22)]),
                detail(1, vec![switch(12, 12, 13)]),
            ],
            clocks,
        );

        let (temp, imported) = import(&fixture(trace));

        assert!(imported.unsupported_plugins().is_empty());
        let dataset = imported.path();
        assert!(dataset.join(".kat-dataset").is_file());
        let rows = batches(&dataset.join("tables/sched_switch.parquet"));
        assert_eq!(rows.iter().map(RecordBatch::num_rows).sum::<usize>(), 5);
        assert_eq!(
            rows.iter()
                .flat_map(|batch| batch
                    .column(0)
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .unwrap()
                    .iter())
                .map(Option::unwrap)
                .collect::<Vec<_>>(),
            ["boottime"; 5]
        );
        assert_eq!(
            rows.iter()
                .flat_map(|batch| batch
                    .column(2)
                    .as_any()
                    .downcast_ref::<UInt32Array>()
                    .unwrap()
                    .values()
                    .iter()
                    .copied())
                .collect::<Vec<_>>(),
            [1, 1, 0, 0, 1]
        );
        assert_eq!(
            rows.iter()
                .flat_map(|batch| batch
                    .column(3)
                    .as_any()
                    .downcast_ref::<UInt64Array>()
                    .unwrap()
                    .values()
                    .iter()
                    .copied())
                .collect::<Vec<_>>(),
            [0, 1, 0, 1, 2]
        );
        let snapshots = batches(&dataset.join("tables/clock_snapshot.parquet"));
        let snapshot_rows = snapshots.iter().map(RecordBatch::num_rows).sum::<usize>();
        assert_eq!(snapshot_rows, 7);
        assert!(
            snapshots
                .iter()
                .flat_map(|batch| {
                    batch
                        .column(2)
                        .as_any()
                        .downcast_ref::<UInt64Array>()
                        .unwrap()
                        .values()
                        .iter()
                        .copied()
                })
                .any(|value| value == 2_000_000_003)
        );
        drop(temp);
    }

    #[test]
    fn local_clock_creates_per_cpu_domains_and_batches_more_than_one_record_batch() {
        let mut events = Vec::new();
        let mut previous = 0;
        for sequence in 0..BATCH_ROWS + 1 {
            let next = i32::try_from(sequence + 1).unwrap();
            events.push(switch(sequence as u64, previous, next));
            previous = next;
        }
        let trace = result(
            "local",
            vec![detail(3, events), detail(4, vec![switch(1, 0, 9)])],
            Vec::new(),
        );

        let (_temp, imported) = import(&fixture(trace));

        let rows = batches(&imported.path().join("tables/sched_switch.parquet"));
        assert!(rows.len() >= 2);
        assert_eq!(
            rows.iter().map(RecordBatch::num_rows).sum::<usize>(),
            BATCH_ROWS + 2
        );
        let domains = batches(&imported.path().join("tables/clock_domain.parquet"));
        let domains = domains[0]
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert!(
            domains
                .iter()
                .flatten()
                .any(|value| value == "ftrace_local_cpu_3")
        );
        assert!(
            domains
                .iter()
                .flatten()
                .any(|value| value == "ftrace_local_cpu_4")
        );
    }

    #[test]
    fn clock_and_thread_continuity_damage_fail_before_target_mutation() {
        for (events, expected) in [
            (
                vec![switch(2, 0, 1), switch(1, 1, 2)],
                "clock went backwards",
            ),
            (
                vec![switch(1, 0, 1), switch(2, 7, 2)],
                "thread continuity is broken",
            ),
        ] {
            let temp = tempdir().unwrap();
            let source = temp.path().join("capture.htrace");
            fs::write(
                &source,
                fixture(result("mono", vec![detail(0, events)], Vec::new())),
            )
            .unwrap();
            let target = temp.path().join("dataset");
            fs::create_dir(&target).unwrap();
            fs::write(target.join("sentinel"), "unchanged").unwrap();

            let error =
                import_hitrace(
                    &source,
                    DatasetWriteTarget::permanently_replace_all_contents(&target),
                )
                .unwrap_err();

            assert!(error.to_string().contains(expected), "{error}");
            assert_eq!(
                fs::read_to_string(target.join("sentinel")).unwrap(),
                "unchanged"
            );
        }
    }

    #[test]
    fn each_loss_evidence_rejects_the_complete_import() {
        for counter in ["overrun", "commit_overrun", "dropped_events", "overwrite"] {
            let mut trace = result("global", vec![detail(0, vec![switch(1, 0, 1)])], Vec::new());
            match counter {
                "overrun" => trace.ftrace_cpu_stats[1].per_cpu_stats[0].overrun = 1,
                "commit_overrun" => trace.ftrace_cpu_stats[1].per_cpu_stats[0].commit_overrun = 1,
                "dropped_events" => trace.ftrace_cpu_stats[1].per_cpu_stats[0].dropped_events = 1,
                "overwrite" => trace.ftrace_cpu_detail[0].overwrite = 1,
                _ => unreachable!(),
            }

            let error = failure(trace);

            assert!(error.to_string().contains(counter), "{counter}: {error}");
        }
    }

    #[test]
    fn trace_start_counters_are_not_used_as_a_baseline() {
        let mut trace = result("boot", vec![detail(0, vec![switch(1, 0, 1)])], Vec::new());
        trace.ftrace_cpu_stats[0].per_cpu_stats[0].overrun = 100;
        trace.ftrace_cpu_stats[0].per_cpu_stats[0].commit_overrun = 100;
        trace.ftrace_cpu_stats[0].per_cpu_stats[0].dropped_events = 100;

        import(&fixture(trace));
    }

    #[test]
    fn capture_requires_one_complete_end_snapshot_and_one_clock() {
        let base = || result("boot", vec![detail(0, vec![switch(1, 0, 1)])], Vec::new());

        let mut missing_end = base();
        missing_end.ftrace_cpu_stats.pop();
        assert!(matches!(
            failure(missing_end),
            HitraceImportError::MissingEndStats
        ));

        let mut duplicate_end = base();
        duplicate_end
            .ftrace_cpu_stats
            .push(stats(StatsStatus::TraceEnd, "boot", &[0]));
        assert!(matches!(
            failure(duplicate_end),
            HitraceImportError::DuplicateEndStats
        ));

        let mut incomplete_end = base();
        incomplete_end.ftrace_cpu_stats[1].per_cpu_stats.clear();
        assert!(matches!(
            failure(incomplete_end),
            HitraceImportError::IncompleteEndStats { cpu: 0 }
        ));

        let mut missing_clock = base();
        for stats in &mut missing_clock.ftrace_cpu_stats {
            stats.trace_clock.clear();
        }
        assert!(matches!(
            failure(missing_clock),
            HitraceImportError::MissingTraceClock
        ));

        let mut conflict = base();
        conflict.ftrace_cpu_stats[1].trace_clock = "mono".to_owned();
        assert!(matches!(
            failure(conflict),
            HitraceImportError::ConflictingTraceClocks { .. }
        ));
    }

    #[test]
    fn duplicate_snapshot_domain_and_unknown_clock_fail() {
        let duplicate = vec![
            ClockDetailMsg {
                id: ClockId::Realtime as i32,
                time: Some(TimeSpec {
                    tv_sec: 1,
                    tv_nsec: 0,
                }),
            },
            ClockDetailMsg {
                id: ClockId::Realtime as i32,
                time: Some(TimeSpec {
                    tv_sec: 2,
                    tv_nsec: 0,
                }),
            },
        ];
        assert!(matches!(
            failure(result("boot", Vec::new(), duplicate)),
            HitraceImportError::DuplicateSnapshotClock { .. }
        ));
        assert!(matches!(
            failure(result(
                "unknown",
                vec![detail(0, vec![switch(1, 0, 1)])],
                Vec::new()
            )),
            HitraceImportError::UnsupportedTraceClock { .. }
        ));
    }

    #[test]
    fn truncated_frame_with_huge_declared_length_fails_without_eager_allocation() {
        let mut bytes = vec![0; HEADER_SIZE];
        bytes[0..8].copy_from_slice(&HEADER_MAGIC.to_le_bytes());
        let declared = HEADER_SIZE as u64 + 4 + u64::from(u32::MAX);
        bytes[8..16].copy_from_slice(&declared.to_le_bytes());
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());

        let error = decode(&bytes)
            .err()
            .expect("expected truncated frame failure");

        assert!(matches!(error, HitraceImportError::InvalidContainer { .. }));
        assert!(error.to_string().contains("truncated protobuf frame"));
    }

    #[test]
    fn unknown_plugins_and_sections_are_sorted_without_becoming_tables() {
        let mut bytes = section(
            PROTOBUF_SECTION,
            &[
                envelope("z-plugin", vec![0xff]),
                envelope("a-plugin", vec![0xff]),
                envelope("z-plugin", vec![0xff]),
            ],
        );
        bytes.extend(section(1000, &[]));
        bytes.extend(section(77, &[]));

        let (_temp, imported) = import(&bytes);

        assert_eq!(imported.unsupported_plugins(), ["a-plugin", "z-plugin"]);
        assert_eq!(imported.unsupported_section_types(), [77, 1000]);
        assert_eq!(imported.unsupported_content().len(), 5);
        assert!(!imported.path().join("tables/z_plugin.parquet").exists());
    }

    #[test]
    #[ignore = "requires KAT_REAL_HITRACE to name a real OpenHarmony zero-loss capture"]
    fn real_openharmony_capture_smoke() {
        let source = PathBuf::from(
            std::env::var_os("KAT_REAL_HITRACE")
                .expect("set KAT_REAL_HITRACE to a real OpenHarmony capture"),
        );
        let temp = tempdir().unwrap();

        let imported = import_hitrace(
            &source,
            DatasetWriteTarget::write_to_empty(temp.path().join("dataset")),
        )
        .unwrap();

        let inspection = crate::inspect_dataset(imported.path()).unwrap();
        assert!(
            inspection
                .tables()
                .iter()
                .any(|table| table.name() == "sched_switch")
        );
    }
}
