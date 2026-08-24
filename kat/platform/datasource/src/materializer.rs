use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs::File,
    io::{self, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, anyhow, bail};
use arrow_array::{
    Array, ArrayRef, Int32Array, RecordBatch, StringArray, StructArray, UInt32Array, UInt64Array,
    builder::LargeStringBuilder,
};
use arrow_schema::{DataType, Field, FieldRef, Schema};
use datafusion::{
    datasource::file_format::file_compression_type::FileCompressionType,
    prelude::{JsonReadOptions, SessionContext},
};
use futures::StreamExt;
use parquet::arrow::{
    ArrowWriter,
    arrow_reader::{
        ArrowReaderMetadata, ArrowReaderOptions, ParquetRecordBatchReader,
        ParquetRecordBatchReaderBuilder,
    },
};

use crate::{
    arrow_table::ArrowTable,
    dataset::{DatasetTableWriter, DatasetWriter},
    dataset_writer::{DatasetPublication, DatasetTableFactory, DatasetWriteTarget},
    domains::ftrace::{FtraceCaptureRecord, FtraceRecord},
    formats::{hitrace, langfuse},
    proto::kat::hitrace::FtraceCpuStatsMsg,
    protobuf_source::{BufferOptions, native_hook::NativeHookSourceCapture},
    record::{TraceRecord, TraceRecordSink},
    sinks::arrow::ArrowSink,
};

const HITRACE_DATASET_FLUSH_RECORDS: usize = 64 * 1024;
const HITRACE_IMPORT_BATCH_ROWS: usize = 8192;
const TICKS_PER_SECOND: u64 = 1_000_000_000;

#[derive(Debug)]
pub struct ImportedHitrace {
    path: PathBuf,
    unsupported_plugins: Vec<String>,
    unsupported_section_types: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct UnsupportedHitraceContent {
    kind: &'static str,
    value: String,
    byte_offset: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum HitraceImportError {
    #[error("{source}")]
    Import {
        #[source]
        source: anyhow::Error,
    },
    #[error("failed to report unsupported Hitrace content")]
    ObserveUnsupportedContent {
        #[source]
        source: io::Error,
    },
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

impl HitraceImportError {
    fn import(source: anyhow::Error) -> Self {
        Self::Import { source }
    }
}

pub fn import_hitrace(
    path: impl AsRef<Path>,
    target: DatasetWriteTarget,
    mut observe_unsupported: impl FnMut(&UnsupportedHitraceContent) -> io::Result<()>,
) -> std::result::Result<ImportedHitrace, HitraceImportError> {
    import_hitrace_inner(path.as_ref(), target, &mut observe_unsupported)
}

fn import_hitrace_inner(
    path: &Path,
    target: DatasetWriteTarget,
    observe_unsupported: &mut impl FnMut(&UnsupportedHitraceContent) -> io::Result<()>,
) -> std::result::Result<ImportedHitrace, HitraceImportError> {
    let publication = DatasetPublication::stage(target)
        .map_err(anyhow::Error::from)
        .map_err(HitraceImportError::import)?;
    let mut sink = LongTermHitraceSink::new();
    let mut source_capture =
        NativeHookSourceCapture::new(BufferOptions::default(), publication.table_factory())
            .map_err(HitraceImportError::import)?;
    let mut observer_failure = None;
    let decoded_report = {
        let mut claim =
            |envelope: &hitrace::profiler::PluginEnvelope<'_>| source_capture.try_claim(envelope);
        let mut observe = |content: &hitrace::UnsupportedHitraceContent| {
            let content = UnsupportedHitraceContent {
                kind: content.kind,
                value: content.value.clone(),
                byte_offset: content.byte_offset,
            };
            if let Err(source) = observe_unsupported(&content) {
                observer_failure = Some(source);
                bail!("unsupported Hitrace content observer failed");
            }
            Ok(())
        };
        hitrace::decode_file_with_report(path, &mut sink, &mut claim, &mut observe)
    };
    if let Some(source) = observer_failure {
        return Err(HitraceImportError::ObserveUnsupportedContent { source });
    }
    let report = match decoded_report {
        Ok(report) => report,
        Err(failure) => {
            return Err(HitraceImportError::import(failure.source.context(format!(
                "failed to decode hitrace file: {}",
                path.display()
            ))));
        }
    };
    let decoded = sink.finish(report).map_err(HitraceImportError::import)?;
    source_capture
        .finish()
        .context("failed to close staged profiler Source tables")
        .map_err(HitraceImportError::import)?;
    let prepared = decoded.prepare().map_err(HitraceImportError::import)?;
    prepared
        .publish(publication)
        .map_err(HitraceImportError::import)
}

struct DecodedLongTermHitrace {
    switches: Option<SwitchSpool>,
    clock_snapshots: Option<ClockSnapshotSpool>,
    header_clock_snapshots: Vec<ClockSnapshot>,
    clock_domains: BTreeMap<String, String>,
    ftrace_clock: Option<FtraceClock>,
    unsupported_plugins: Vec<String>,
    unsupported_section_types: Vec<u32>,
}

/// 只允许已经完成 decode、领域校验与临时 spool 预读的事实进入 Dataset 写事务。
struct PreparedImport {
    clock_domains: RecordBatch,
    header_clock_snapshots: Vec<RecordBatch>,
    clock_snapshots: Option<ParquetRecordBatchReader>,
    sched_switches: Option<(ParquetRecordBatchReader, FtraceClock)>,
    unsupported_plugins: Vec<String>,
    unsupported_section_types: Vec<u32>,
}

impl DecodedLongTermHitrace {
    fn prepare(self) -> Result<PreparedImport> {
        let clock_domains = clock_domain_batch(&self.clock_domains)?;
        let header_clock_snapshots = self
            .header_clock_snapshots
            .chunks(HITRACE_IMPORT_BATCH_ROWS)
            .map(clock_snapshot_batch)
            .collect::<Result<Vec<_>>>()?;
        let clock_snapshots = self
            .clock_snapshots
            .map(ClockSnapshotSpool::prepare)
            .transpose()?;
        let sched_switches = self
            .switches
            .map(|switches| -> Result<_> {
                let clock = self
                    .ftrace_clock
                    .expect("switches require a validated clock");
                Ok((switches.prepare()?, clock))
            })
            .transpose()?;

        Ok(PreparedImport {
            clock_domains,
            header_clock_snapshots,
            clock_snapshots,
            sched_switches,
            unsupported_plugins: self.unsupported_plugins,
            unsupported_section_types: self.unsupported_section_types,
        })
    }
}

impl PreparedImport {
    fn publish(self, publication: DatasetPublication) -> Result<ImportedHitrace> {
        let tables = publication.table_factory();
        write_clock_domains(&tables, self.clock_domains)?;
        write_clock_snapshots(&tables, self.header_clock_snapshots, self.clock_snapshots)?;
        if let Some((switches, clock)) = self.sched_switches {
            write_sched_switches(&tables, switches, clock)?;
        }
        let path = publication.publish()?;

        Ok(ImportedHitrace {
            path,
            unsupported_plugins: self.unsupported_plugins,
            unsupported_section_types: self.unsupported_section_types,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct FtraceClock {
    label: &'static str,
    domain: &'static str,
    clock_type: &'static str,
    cpu_scoped: bool,
}

const FTRACE_CLOCKS: [FtraceClock; 4] = [
    FtraceClock {
        label: "boot",
        domain: "boottime",
        clock_type: "boottime",
        cpu_scoped: false,
    },
    FtraceClock {
        label: "mono",
        domain: "monotonic",
        clock_type: "monotonic",
        cpu_scoped: false,
    },
    FtraceClock {
        label: "global",
        domain: "ftrace_global",
        clock_type: "ftrace_global",
        cpu_scoped: false,
    },
    FtraceClock {
        label: "local",
        domain: "ftrace_local",
        clock_type: "ftrace_local",
        cpu_scoped: true,
    },
];

impl FtraceClock {
    fn parse(value: &str) -> Result<Self> {
        let value = value.trim();
        FTRACE_CLOCKS
            .iter()
            .copied()
            .find(|clock| clock.label == value)
            .with_context(|| format!("unsupported Hitrace trace clock {value:?}"))
    }

    fn domain(self, cpu: u32) -> String {
        if self.cpu_scoped {
            format!("{}_cpu_{cpu}", self.domain)
        } else {
            self.domain.to_owned()
        }
    }
}

struct ClockSnapshot {
    snapshot_id: u64,
    clock_domain: String,
    clock_value: u64,
}

struct LongTermHitraceSink {
    switches: Option<SwitchSpool>,
    clock_snapshots: Option<ClockSnapshotSpool>,
    reported_clocks: BTreeSet<FtraceClock>,
    first_clock_error: Option<anyhow::Error>,
    capture_integrity_error: Option<anyhow::Error>,
    end_stats_cpus: Option<HashSet<u32>>,
    detail_cpus: HashMap<u32, u64>,
    first_nonzero_overwrite: Option<(u64, u32, u64)>,
    next_detail_sequence: u64,
    next_snapshot_id: u64,
    last_switch: HashMap<u32, CpuSwitchState>,
}

struct CpuSwitchState {
    clock_value: u64,
    next_thread_id: i32,
    sequence: u64,
}

impl LongTermHitraceSink {
    fn new() -> Self {
        Self {
            switches: None,
            clock_snapshots: None,
            reported_clocks: BTreeSet::new(),
            first_clock_error: None,
            capture_integrity_error: None,
            end_stats_cpus: None,
            detail_cpus: HashMap::new(),
            first_nonzero_overwrite: None,
            next_detail_sequence: 0,
            next_snapshot_id: 1,
            last_switch: HashMap::new(),
        }
    }

    fn push_capture(&mut self, record: FtraceCaptureRecord) -> Result<()> {
        match record {
            FtraceCaptureRecord::CpuStats(stats) => {
                self.push_capture_stats(stats);
            }
            FtraceCaptureRecord::ClockSnapshot(clocks) => {
                let snapshot_id = self.next_snapshot_id;
                self.next_snapshot_id = self
                    .next_snapshot_id
                    .checked_add(1)
                    .context("Hitrace snapshot id overflows")?;
                let mut domains = HashSet::new();
                for clock in clocks {
                    let domain = snapshot_clock_domain(clock.id)?;
                    if !domains.insert(domain) {
                        bail!("clock snapshot {snapshot_id} repeats domain {domain:?}");
                    }
                    let time = clock.time.with_context(|| {
                        format!("clock snapshot {snapshot_id} has no time for domain {domain:?}")
                    })?;
                    if time.tv_nsec >= TICKS_PER_SECOND as u32 {
                        bail!(
                            "clock snapshot {snapshot_id} has invalid nanoseconds {} for domain {domain:?}",
                            time.tv_nsec
                        );
                    }
                    let clock_value = u64::from(time.tv_sec)
                        .checked_mul(TICKS_PER_SECOND)
                        .and_then(|value| value.checked_add(u64::from(time.tv_nsec)))
                        .with_context(|| {
                            format!("clock snapshot {snapshot_id} overflows UInt64 for domain {domain:?}")
                        })?;
                    if self.clock_snapshots.is_none() {
                        self.clock_snapshots = Some(ClockSnapshotSpool::new()?);
                    }
                    self.clock_snapshots
                        .as_mut()
                        .expect("clock snapshot spool is initialized")
                        .push(ClockSnapshot {
                            snapshot_id,
                            clock_domain: domain.to_owned(),
                            clock_value,
                        })?;
                }
            }
            FtraceCaptureRecord::CpuDetail { cpu, overwrite } => {
                let sequence = self.next_detail_sequence;
                self.next_detail_sequence = self
                    .next_detail_sequence
                    .checked_add(1)
                    .context("Hitrace CPU detail sequence overflows")?;
                self.detail_cpus.entry(cpu).or_insert(sequence);
                if overwrite != 0 && self.first_nonzero_overwrite.is_none() {
                    self.first_nonzero_overwrite = Some((sequence, cpu, overwrite));
                }
            }
        }
        Ok(())
    }

    fn push_capture_stats(&mut self, stats: FtraceCpuStatsMsg) {
        if !stats.trace_clock.trim().is_empty() {
            match FtraceClock::parse(&stats.trace_clock) {
                Ok(clock) => {
                    self.reported_clocks.insert(clock);
                }
                Err(source) if self.first_clock_error.is_none() => {
                    self.first_clock_error = Some(source);
                }
                Err(_) => {}
            }
        }

        if self.capture_integrity_error.is_some() {
            return;
        }
        if !matches!(stats.status, 0 | 1) {
            self.capture_integrity_error =
                Some(anyhow!("invalid ftrace stats status {}", stats.status));
            return;
        }
        if stats.status != 1 {
            return;
        }
        if self.end_stats_cpus.is_some() {
            self.capture_integrity_error = Some(anyhow!("duplicate ftrace TRACE_END statistics"));
            return;
        }

        let mut cpus = HashSet::new();
        for cpu_stats in &stats.per_cpu_stats {
            let cpu = match u32::try_from(cpu_stats.cpu) {
                Ok(cpu) => cpu,
                Err(_) => {
                    self.capture_integrity_error = Some(anyhow!(
                        "ftrace CPU id {} cannot be represented as UInt32",
                        cpu_stats.cpu
                    ));
                    return;
                }
            };
            if !cpus.insert(cpu) {
                self.capture_integrity_error = Some(anyhow!(
                    "duplicate ftrace TRACE_END statistics for CPU {cpu}"
                ));
                return;
            }
            if cpu_stats.overrun != 0
                || cpu_stats.commit_overrun != 0
                || cpu_stats.dropped_events != 0
            {
                self.capture_integrity_error = Some(anyhow!(
                    "ftrace capture lost events on CPU {cpu}: overrun={}, commit_overrun={}, dropped_events={}",
                    cpu_stats.overrun,
                    cpu_stats.commit_overrun,
                    cpu_stats.dropped_events
                ));
                return;
            }
        }
        self.end_stats_cpus = Some(cpus);
    }

    fn push_ftrace(&mut self, record: FtraceRecord) -> Result<()> {
        let FtraceRecord::Event(event) = record;
        let event = *event;
        let Some(switch) = event.event.sched_switch_format else {
            return Ok(());
        };
        let cpu = event.context.cpu;
        let clock_value = event.context.timestamp;
        let sequence = match self.last_switch.get(&cpu) {
            None => 0,
            Some(previous) => {
                if clock_value < previous.clock_value {
                    bail!(
                        "sched_switch clock went backwards on CPU {cpu}: {} then {clock_value}",
                        previous.clock_value
                    );
                }
                if switch.prev_pid != previous.next_thread_id {
                    bail!(
                        "sched_switch thread continuity is broken on CPU {cpu}: expected previous_thread_id {}, got {}",
                        previous.next_thread_id,
                        switch.prev_pid
                    );
                }
                previous
                    .sequence
                    .checked_add(1)
                    .with_context(|| format!("sched_switch sequence overflows on CPU {cpu}"))?
            }
        };
        self.last_switch.insert(
            cpu,
            CpuSwitchState {
                clock_value,
                next_thread_id: switch.next_pid,
                sequence,
            },
        );
        if self.switches.is_none() {
            self.switches = Some(SwitchSpool::new()?);
        }
        self.switches
            .as_mut()
            .expect("switch spool is initialized")
            .push(SwitchRow {
                clock_value,
                cpu,
                sequence,
                previous_thread_id: switch.prev_pid,
                previous_thread_name: switch.prev_comm,
                next_thread_id: switch.next_pid,
                next_thread_name: switch.next_comm,
            })
    }

    fn finish(mut self, report: hitrace::HitraceDecodeReport) -> Result<DecodedLongTermHitrace> {
        let ftrace_clock = self.validate_ftrace_capture()?;

        let mut clock_domains = report.clock_domains;
        if let Some(clock) = ftrace_clock {
            for cpu in self.last_switch.keys() {
                clock_domains
                    .entry(clock.domain(*cpu))
                    .or_insert_with(|| clock.clock_type.to_owned());
            }
        }
        let header_clock_snapshots = report
            .clock_snapshots
            .into_iter()
            .map(|snapshot| ClockSnapshot {
                snapshot_id: snapshot.snapshot_id,
                clock_domain: snapshot.clock_domain,
                clock_value: snapshot.clock_value,
            })
            .collect::<Vec<_>>();

        Ok(DecodedLongTermHitrace {
            switches: self.switches,
            clock_snapshots: self.clock_snapshots,
            header_clock_snapshots,
            clock_domains,
            ftrace_clock,
            unsupported_plugins: report.unsupported_plugins.into_iter().collect(),
            unsupported_section_types: report.unsupported_section_types.into_iter().collect(),
        })
    }

    fn validate_ftrace_capture(&mut self) -> Result<Option<FtraceClock>> {
        if let Some(source) = self.first_clock_error.take() {
            return Err(source);
        }
        let ftrace_clock = match self.reported_clocks.len() {
            0 => None,
            count if count > 1 => {
                let clocks = self
                    .reported_clocks
                    .iter()
                    .map(|clock| clock.label)
                    .collect::<Vec<_>>();
                bail!("Hitrace reports conflicting ftrace clocks: {clocks:?}");
            }
            1 => self.reported_clocks.first().copied(),
            _ => unreachable!(),
        };
        if self.switches.is_none() {
            return Ok(None);
        }
        let ftrace_clock = ftrace_clock.context("Hitrace sched_switch data has no ftrace clock")?;
        if let Some(source) = self.capture_integrity_error.take() {
            return Err(source);
        }
        let end_stats = self
            .end_stats_cpus
            .as_ref()
            .context("Hitrace sched_switch data has no TRACE_END statistics")?;
        let first_missing_cpu = self
            .detail_cpus
            .iter()
            .filter(|(cpu, _)| !end_stats.contains(cpu))
            .min_by_key(|(_, sequence)| *sequence)
            .map(|(cpu, sequence)| (*sequence, *cpu));
        match (self.first_nonzero_overwrite, first_missing_cpu) {
            (Some((sequence, cpu, overwrite)), Some((missing_sequence, _)))
                if sequence <= missing_sequence =>
            {
                bail!("ftrace page overwrite is nonzero on CPU {cpu}: {overwrite}");
            }
            (_, Some((_, cpu))) => {
                bail!("Hitrace TRACE_END statistics are missing CPU {cpu}");
            }
            (Some((_, cpu, overwrite)), None) => {
                bail!("ftrace page overwrite is nonzero on CPU {cpu}: {overwrite}");
            }
            (None, None) => {}
        }
        Ok(Some(ftrace_clock))
    }
}

impl TraceRecordSink for LongTermHitraceSink {
    fn push(&mut self, record: TraceRecord) -> Result<()> {
        match record {
            TraceRecord::FtraceCapture(record) => self.push_capture(record),
            TraceRecord::Ftrace(record) => self.push_ftrace(*record),
            TraceRecord::ProfilerPluginData(_) | TraceRecord::NativeHook(_) => Ok(()),
        }
    }
}

fn snapshot_clock_domain(id: i32) -> Result<&'static str> {
    match id {
        1 => Ok("boottime"),
        2 => Ok("realtime"),
        3 => Ok("realtime_coarse"),
        4 => Ok("monotonic"),
        5 => Ok("monotonic_coarse"),
        6 => Ok("monotonic_raw"),
        id => bail!("unsupported Hitrace snapshot clock id {id}"),
    }
}

struct ClockSnapshotSpool {
    writer: ArrowWriter<File>,
    rows: Vec<ClockSnapshot>,
    total_rows: usize,
}

impl ClockSnapshotSpool {
    fn new() -> Result<Self> {
        let file = tempfile::tempfile().context("failed to create bounded clock snapshot spool")?;
        Ok(Self {
            writer: ArrowWriter::try_new(file, clock_snapshot_schema(), None)
                .context("failed to open clock snapshot Parquet spool")?,
            rows: Vec::with_capacity(HITRACE_IMPORT_BATCH_ROWS),
            total_rows: 0,
        })
    }

    fn push(&mut self, row: ClockSnapshot) -> Result<()> {
        self.rows.push(row);
        self.total_rows = self
            .total_rows
            .checked_add(1)
            .context("clock snapshot spool row count overflows")?;
        if self.rows.len() >= HITRACE_IMPORT_BATCH_ROWS {
            self.flush_rows()?;
        }
        Ok(())
    }

    fn flush_rows(&mut self) -> Result<()> {
        if self.rows.is_empty() {
            return Ok(());
        }
        let rows = std::mem::take(&mut self.rows);
        self.writer
            .write(&clock_snapshot_batch(&rows)?)
            .context("failed to write clock snapshot Parquet spool")
    }

    fn prepare(mut self) -> Result<ParquetRecordBatchReader> {
        self.flush_rows()?;
        let file = self
            .writer
            .into_inner()
            .context("failed to finish clock snapshot Parquet spool")?;
        prepare_spool_reader(
            file,
            clock_snapshot_schema(),
            self.total_rows,
            "clock snapshot",
        )
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
    writer: ArrowWriter<File>,
    rows: Vec<SwitchRow>,
    total_rows: usize,
}

impl SwitchSpool {
    fn new() -> Result<Self> {
        let file = tempfile::tempfile().context("failed to create bounded sched_switch spool")?;
        Ok(Self {
            writer: ArrowWriter::try_new(file, switch_spool_schema(), None)
                .context("failed to open sched_switch Parquet spool")?,
            rows: Vec::with_capacity(HITRACE_IMPORT_BATCH_ROWS),
            total_rows: 0,
        })
    }

    fn push(&mut self, row: SwitchRow) -> Result<()> {
        self.rows.push(row);
        self.total_rows = self
            .total_rows
            .checked_add(1)
            .context("sched_switch spool row count overflows")?;
        if self.rows.len() >= HITRACE_IMPORT_BATCH_ROWS {
            self.flush_rows()?;
        }
        Ok(())
    }

    fn flush_rows(&mut self) -> Result<()> {
        if self.rows.is_empty() {
            return Ok(());
        }
        let rows = std::mem::take(&mut self.rows);
        let batch = RecordBatch::try_new(
            switch_spool_schema(),
            vec![
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
        )?;
        self.writer
            .write(&batch)
            .context("failed to write sched_switch Parquet spool")
    }

    fn prepare(mut self) -> Result<ParquetRecordBatchReader> {
        self.flush_rows()?;
        let file = self
            .writer
            .into_inner()
            .context("failed to finish sched_switch Parquet spool")?;
        prepare_spool_reader(file, switch_spool_schema(), self.total_rows, "sched_switch")
    }
}

fn prepare_spool_reader(
    mut file: File,
    expected_schema: Arc<Schema>,
    expected_rows: usize,
    label: &str,
) -> Result<ParquetRecordBatchReader> {
    file.seek(SeekFrom::Start(0))
        .with_context(|| format!("failed to rewind {label} spool"))?;
    let metadata = ArrowReaderMetadata::load(&file, ArrowReaderOptions::default())
        .with_context(|| format!("failed to read {label} Parquet spool metadata"))?;
    if metadata.schema().as_ref() != expected_schema.as_ref() {
        bail!(
            "{label} Parquet spool schema differs from the planned schema: planned={expected_schema:?} actual={:?}",
            metadata.schema()
        );
    }
    let actual_rows = usize::try_from(metadata.metadata().file_metadata().num_rows())
        .with_context(|| format!("{label} Parquet spool has an invalid row count"))?;
    if actual_rows != expected_rows {
        bail!(
            "{label} Parquet spool row count differs: expected {expected_rows}, actual {actual_rows}"
        );
    }

    let mut preflight_rows = 0_usize;
    for row_group in 0..metadata.metadata().num_row_groups() {
        let mut reader_file = file
            .try_clone()
            .with_context(|| format!("failed to clone {label} spool row group {row_group}"))?;
        reader_file
            .seek(SeekFrom::Start(0))
            .with_context(|| format!("failed to rewind {label} spool row group {row_group}"))?;
        let mut reader =
            ParquetRecordBatchReaderBuilder::new_with_metadata(reader_file, metadata.clone())
                .with_row_groups(vec![row_group])
                .with_batch_size(HITRACE_IMPORT_BATCH_ROWS)
                .build()
                .with_context(|| format!("failed to open {label} spool row group {row_group}"))?;
        for batch in &mut reader {
            let batch = batch.with_context(|| {
                format!("failed to preflight {label} spool row group {row_group}")
            })?;
            if batch.schema().as_ref() != expected_schema.as_ref() {
                bail!(
                    "{label} spool batch schema differs in row group {row_group}: planned={expected_schema:?} actual={:?}",
                    batch.schema()
                );
            }
            preflight_rows = preflight_rows
                .checked_add(batch.num_rows())
                .with_context(|| format!("{label} spool preflight row count overflows"))?;
        }
    }
    if preflight_rows != expected_rows {
        bail!(
            "{label} spool preflight row count differs: expected {expected_rows}, actual {preflight_rows}"
        );
    }

    file.seek(SeekFrom::Start(0))
        .with_context(|| format!("failed to rewind preflighted {label} spool"))?;
    ParquetRecordBatchReaderBuilder::new_with_metadata(file, metadata)
        .with_batch_size(HITRACE_IMPORT_BATCH_ROWS)
        .build()
        .with_context(|| format!("failed to open preflighted {label} spool reader"))
}

fn switch_spool_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("clock_value", DataType::UInt64, false),
        Field::new("cpu", DataType::UInt32, false),
        Field::new("cpu_switch_sequence", DataType::UInt64, false),
        Field::new("previous_thread_id", DataType::Int32, false),
        Field::new("previous_thread_name", DataType::Utf8, false),
        Field::new("next_thread_id", DataType::Int32, false),
        Field::new("next_thread_name", DataType::Utf8, false),
    ]))
}

fn sched_switch_schema() -> Arc<Schema> {
    let spool_schema = switch_spool_schema();
    let mut fields = Vec::with_capacity(spool_schema.fields().len() + 1);
    fields.push(Arc::new(Field::new("clock_domain", DataType::Utf8, false)));
    fields.extend(spool_schema.fields().iter().cloned());
    Arc::new(Schema::new(fields))
}

fn clock_snapshot_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("snapshot_id", DataType::UInt64, false),
        Field::new("clock_domain", DataType::Utf8, false),
        Field::new("clock_value", DataType::UInt64, false),
    ]))
}

fn clock_snapshot_batch(rows: &[ClockSnapshot]) -> Result<RecordBatch> {
    Ok(RecordBatch::try_new(
        clock_snapshot_schema(),
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
    )?)
}

fn clock_domain_batch(domains: &BTreeMap<String, String>) -> Result<RecordBatch> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("clock_domain", DataType::Utf8, false),
        Field::new("clock_type", DataType::Utf8, false),
        Field::new("ticks_per_second", DataType::UInt64, false),
    ]));
    Ok(RecordBatch::try_new(
        Arc::clone(&schema),
        vec![
            Arc::new(StringArray::from_iter_values(domains.keys())),
            Arc::new(StringArray::from_iter_values(domains.values())),
            Arc::new(UInt64Array::from(vec![TICKS_PER_SECOND; domains.len()])),
        ],
    )?)
}

fn write_clock_domains(writer: &DatasetTableFactory, batch: RecordBatch) -> Result<()> {
    let mut table = writer.begin_table("clock_domain", batch.schema())?;
    table.write(&batch)?;
    table.finish()?;
    Ok(())
}

fn write_clock_snapshots(
    writer: &DatasetTableFactory,
    header_snapshots: Vec<RecordBatch>,
    mut snapshots: Option<ParquetRecordBatchReader>,
) -> Result<()> {
    let schema = clock_snapshot_schema();
    let mut table = writer.begin_table("clock_snapshot", Arc::clone(&schema))?;
    for batch in &header_snapshots {
        table.write(batch)?;
    }
    if let Some(snapshots) = snapshots.as_mut() {
        for batch in snapshots {
            table.write(&batch.context("failed to read clock snapshot Parquet spool")?)?;
        }
    }
    table.finish()?;
    Ok(())
}

fn write_sched_switches(
    writer: &DatasetTableFactory,
    mut switches: ParquetRecordBatchReader,
    clock: FtraceClock,
) -> Result<()> {
    let schema = sched_switch_schema();
    let mut table = writer.begin_table("sched_switch", Arc::clone(&schema))?;
    for batch in &mut switches {
        let batch = batch.context("failed to read sched_switch Parquet spool")?;
        let cpus = batch
            .column_by_name("cpu")
            .context("sched_switch Parquet spool has no CPU column")?
            .as_any()
            .downcast_ref::<UInt32Array>()
            .context("sched_switch Parquet spool CPU column is not UInt32")?;
        let mut columns = Vec::with_capacity(batch.num_columns() + 1);
        columns.push(Arc::new(StringArray::from_iter_values(
            (0..batch.num_rows()).map(|row| clock.domain(cpus.value(row))),
        )) as ArrayRef);
        columns.extend(batch.columns().iter().cloned());
        table.write(&RecordBatch::try_new(Arc::clone(&schema), columns)?)?;
    }
    table.finish()?;
    Ok(())
}

pub async fn materialize_hitrace_dataset(
    path: impl AsRef<Path>,
    dataset_path: impl AsRef<Path>,
) -> Result<()> {
    let path = path.as_ref();
    let dataset_path = dataset_path.as_ref();

    let writer = DatasetWriter::create(dataset_path)?;
    let mut sink = HitraceDatasetSink::new(writer)?;
    hitrace::decode_file(path, &mut sink)
        .with_context(|| format!("failed to decode hitrace file: {}", path.display()))?;
    let writer = sink.finish()?;
    writer.finish().await
}

pub async fn materialize_langfuse_legacy_dataset(
    observations_path: impl AsRef<Path>,
    traces_path: impl AsRef<Path>,
    dataset_path: impl AsRef<Path>,
) -> Result<()> {
    let observations_path = observations_path.as_ref();
    let traces_path = traces_path.as_ref();
    let dataset_path = dataset_path.as_ref();

    let mut writer = DatasetWriter::create(dataset_path)?;
    write_langfuse_tables(&mut writer, observations_path, traces_path)
        .await
        .with_context(|| {
            format!(
                "failed to write Langfuse legacy dataset tables: {}",
                dataset_path.display()
            )
        })?;
    writer.finish().await
}

async fn write_langfuse_tables(
    writer: &mut DatasetWriter,
    observations_path: &Path,
    traces_path: &Path,
) -> Result<()> {
    for table in langfuse::legacy_json_tables(observations_path, traces_path) {
        write_langfuse_table(writer, table.name, table.path).await?;
    }

    Ok(())
}

struct HitraceDatasetSink {
    arrow_sink: ArrowSink,
    dataset_writer: DatasetWriter,
    table_writers: Vec<OpenHitraceTableWriter>,
    records_since_flush: usize,
}

struct OpenHitraceTableWriter {
    name: &'static str,
    writer: DatasetTableWriter,
}

impl HitraceDatasetSink {
    fn new(dataset_writer: DatasetWriter) -> Result<Self> {
        Ok(Self {
            arrow_sink: ArrowSink::new()?,
            dataset_writer,
            table_writers: Vec::new(),
            records_since_flush: 0,
        })
    }

    fn finish(mut self) -> Result<DatasetWriter> {
        self.flush_tables(true)?;
        self.finish_open_tables()?;
        Ok(self.dataset_writer)
    }

    fn finish_open_tables(&mut self) -> Result<()> {
        for table in std::mem::take(&mut self.table_writers) {
            self.dataset_writer.add_table(table.writer.finish()?);
        }
        Ok(())
    }

    fn flush_tables(&mut self, include_empty_tables: bool) -> Result<()> {
        let tables = self.arrow_sink.flush()?;

        for table in tables.tables {
            let row_count = table_row_count(&table);
            if row_count == 0 {
                let already_open = self
                    .table_writers
                    .iter()
                    .any(|open_table| open_table.name == table.name);
                if already_open || !include_empty_tables {
                    continue;
                }
            }

            let writer = self.table_writer_for(&table)?;
            for batch in &table.batches {
                writer.write(batch)?;
            }
        }

        self.records_since_flush = 0;
        Ok(())
    }

    fn table_writer_for(&mut self, table: &ArrowTable) -> Result<&mut DatasetTableWriter> {
        if let Some(index) = self
            .table_writers
            .iter()
            .position(|open_table| open_table.name == table.name)
        {
            return Ok(&mut self.table_writers[index].writer);
        }

        let first_batch = table
            .batches
            .first()
            .with_context(|| format!("hitrace table {} has no record batches", table.name))?;
        let parquet_file_name = format!("hitrace.{}.parquet", table.name);
        let writer = self.dataset_writer.start_table(
            table.name,
            &parquet_file_name,
            first_batch.schema(),
        )?;
        self.table_writers.push(OpenHitraceTableWriter {
            name: table.name,
            writer,
        });
        let index = self.table_writers.len() - 1;

        Ok(&mut self.table_writers[index].writer)
    }
}

impl TraceRecordSink for HitraceDatasetSink {
    fn push(&mut self, record: TraceRecord) -> Result<()> {
        self.arrow_sink.push(record)?;
        self.records_since_flush += 1;

        if self.records_since_flush >= HITRACE_DATASET_FLUSH_RECORDS {
            self.flush_tables(false)?;
        }

        Ok(())
    }
}

fn table_row_count(table: &ArrowTable) -> usize {
    table
        .batches
        .iter()
        .map(RecordBatch::num_rows)
        .sum::<usize>()
}

async fn write_langfuse_table(
    dataset_writer: &mut DatasetWriter,
    table_name: &str,
    jsonl_path: &Path,
) -> Result<()> {
    let jsonl_path_str = jsonl_path.to_str().with_context(|| {
        format!(
            "Langfuse export path is not valid UTF-8: {}",
            jsonl_path.display()
        )
    })?;
    let staging_ctx = SessionContext::new();
    // Keep parity with the legacy datasource's DataFusion JSON inference; explicit schema is future work.
    let options = JsonReadOptions::default()
        .file_extension(".jsonl.gz")
        .file_compression_type(FileCompressionType::GZIP);

    staging_ctx
        .register_json(table_name, jsonl_path_str, options)
        .await
        .with_context(|| {
            format!("failed to register Langfuse JSONL table {table_name} from {jsonl_path_str}")
        })?;
    let dataframe = staging_ctx.table(table_name).await.with_context(|| {
        format!("failed to read Langfuse JSONL table {table_name} from {jsonl_path_str}")
    })?;
    let mut stream = dataframe.execute_stream().await.with_context(|| {
        format!("failed to stream Langfuse JSONL table {table_name} from {jsonl_path_str}")
    })?;

    let parquet_file_name = format!("langfuse.{table_name}.parquet");
    let mut parquet_writer = None;

    while let Some(batch) = stream.next().await {
        let batch = batch.with_context(|| {
            format!("failed to stream Langfuse JSONL table {table_name} from {jsonl_path_str}")
        })?;
        let batch = parquet_compatible_langfuse_batch(batch)?;

        if parquet_writer.is_none() {
            parquet_writer =
                Some(dataset_writer.start_table(table_name, &parquet_file_name, batch.schema())?);
        }

        parquet_writer
            .as_mut()
            .expect("writer is initialized before writing batches")
            .write(&batch)?;
    }

    let Some(parquet_writer) = parquet_writer else {
        bail!("Langfuse JSONL table {table_name} from {jsonl_path_str} produced no batches");
    };
    dataset_writer.add_table(parquet_writer.finish()?);

    Ok(())
}

fn parquet_compatible_langfuse_batch(batch: RecordBatch) -> Result<RecordBatch> {
    let schema = batch.schema();
    let mut fields = Vec::with_capacity(schema.fields().len());
    let mut columns = Vec::with_capacity(batch.num_columns());
    let mut changed = false;

    for (field, column) in schema.fields().iter().zip(batch.columns()) {
        if matches!(field.data_type(), DataType::Struct(fields) if fields.is_empty()) {
            changed = true;
            fields.push(langfuse_json_string_field(field));
            columns.push(empty_struct_column_to_json(column.as_ref())?);
        } else {
            fields.push(Arc::clone(field));
            columns.push(Arc::clone(column));
        }
    }

    if !changed {
        return Ok(batch);
    }

    let schema = Schema::new_with_metadata(fields, schema.metadata().clone());
    RecordBatch::try_new(Arc::new(schema), columns)
        .context("failed to convert Langfuse empty object columns before Parquet write")
}

fn langfuse_json_string_field(field: &FieldRef) -> FieldRef {
    let mut converted = Field::new(field.name(), DataType::LargeUtf8, field.is_nullable());
    converted.set_metadata(field.metadata().clone());
    Arc::new(converted)
}

fn empty_struct_column_to_json(column: &dyn Array) -> Result<ArrayRef> {
    let struct_column = column
        .as_any()
        .downcast_ref::<StructArray>()
        .context("Langfuse empty object column was not an Arrow struct array")?;
    let mut builder = LargeStringBuilder::with_capacity(column.len(), column.len() * 2);

    for row in 0..column.len() {
        if struct_column.is_null(row) {
            builder.append_null();
        } else {
            builder.append_value("{}");
        }
    }

    Ok(Arc::new(builder.finish()))
}
