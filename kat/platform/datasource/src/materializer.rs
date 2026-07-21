use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs::File,
    io::{Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, bail};
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
    arrow_reader::{ParquetRecordBatchReader, ParquetRecordBatchReaderBuilder},
};

use crate::{
    arrow_table::ArrowTable,
    dataset::{DatasetTableWriter, DatasetWriter},
    dataset_writer::{DatasetWriteTarget, DatasetWriter as ManagedDatasetWriter},
    domains::ftrace::{FtraceCaptureRecord, FtraceRecord},
    formats::{hitrace, langfuse},
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
    unsupported_content: Vec<UnsupportedHitraceContent>,
}

#[derive(Debug)]
pub struct UnsupportedHitraceContent {
    kind: &'static str,
    value: String,
    byte_offset: usize,
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct HitraceImportError(#[from] anyhow::Error);

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
    path: impl AsRef<Path>,
    target: DatasetWriteTarget,
) -> std::result::Result<ImportedHitrace, HitraceImportError> {
    import_hitrace_inner(path.as_ref(), target).map_err(Into::into)
}

fn import_hitrace_inner(path: &Path, target: DatasetWriteTarget) -> Result<ImportedHitrace> {
    // 先让迁移后的 Hitrace format/domain pipeline 完成解析与完整性校验，再授权覆盖目标。
    let mut sink = LongTermHitraceSink::new();
    let report = hitrace::decode_file_with_report(path, &mut sink)
        .with_context(|| format!("failed to decode hitrace file: {}", path.display()))?;
    let decoded = sink.finish(report)?;

    let mut writer = ManagedDatasetWriter::begin(target)?;
    write_clock_domains(&mut writer, &decoded.clock_domains)?;
    write_clock_snapshots(&mut writer, &decoded.clock_snapshots)?;
    if let Some(switches) = decoded.switches {
        write_sched_switches(
            &mut writer,
            switches.into_reader()?,
            decoded
                .ftrace_clock
                .expect("switches require a validated clock"),
        )?;
    }
    let path = writer.finish()?;

    Ok(ImportedHitrace {
        path,
        unsupported_plugins: decoded.unsupported_plugins,
        unsupported_section_types: decoded.unsupported_section_types,
        unsupported_content: decoded.unsupported_content,
    })
}

struct DecodedLongTermHitrace {
    switches: Option<SwitchSpool>,
    clock_snapshots: Vec<ClockSnapshot>,
    clock_domains: BTreeMap<String, String>,
    ftrace_clock: Option<FtraceClock>,
    unsupported_plugins: Vec<String>,
    unsupported_section_types: Vec<u32>,
    unsupported_content: Vec<UnsupportedHitraceContent>,
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
    clock_snapshots: Vec<ClockSnapshot>,
    reported_clocks: BTreeSet<FtraceClock>,
    detail_cpus: BTreeSet<u32>,
    end_stats: Option<HashSet<u32>>,
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
            clock_snapshots: Vec::new(),
            reported_clocks: BTreeSet::new(),
            detail_cpus: BTreeSet::new(),
            end_stats: None,
            next_snapshot_id: 1,
            last_switch: HashMap::new(),
        }
    }

    fn push_capture(&mut self, record: FtraceCaptureRecord) -> Result<()> {
        match record {
            FtraceCaptureRecord::CpuStats(stats) => {
                if !matches!(stats.status, 0 | 1) {
                    bail!("invalid ftrace stats status {}", stats.status);
                }
                if !stats.trace_clock.trim().is_empty() {
                    self.reported_clocks
                        .insert(FtraceClock::parse(&stats.trace_clock)?);
                }
                if stats.status == 1 {
                    if self.end_stats.is_some() {
                        bail!("duplicate ftrace TRACE_END statistics");
                    }
                    let mut end_stats = HashSet::new();
                    for cpu_stats in stats.per_cpu_stats {
                        let cpu = u32::try_from(cpu_stats.cpu).with_context(|| {
                            format!(
                                "ftrace CPU id {} cannot be represented as UInt32",
                                cpu_stats.cpu
                            )
                        })?;
                        if !end_stats.insert(cpu) {
                            bail!("duplicate ftrace TRACE_END statistics for CPU {cpu}");
                        }
                        if cpu_stats.overrun != 0
                            || cpu_stats.commit_overrun != 0
                            || cpu_stats.dropped_events != 0
                        {
                            bail!(
                                "ftrace capture lost events on CPU {cpu}: overrun={}, commit_overrun={}, dropped_events={}",
                                cpu_stats.overrun,
                                cpu_stats.commit_overrun,
                                cpu_stats.dropped_events
                            );
                        }
                    }
                    self.end_stats = Some(end_stats);
                }
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
                    self.clock_snapshots.push(ClockSnapshot {
                        snapshot_id,
                        clock_domain: domain.to_owned(),
                        clock_value,
                    });
                }
            }
            FtraceCaptureRecord::CpuDetail { cpu, overwrite } => {
                self.detail_cpus.insert(cpu);
                if overwrite != 0 {
                    bail!("ftrace page overwrite is nonzero on CPU {cpu}: {overwrite}");
                }
            }
        }
        Ok(())
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

    fn finish(self, report: hitrace::HitraceDecodeReport) -> Result<DecodedLongTermHitrace> {
        let ftrace_clock = match (self.switches.is_some(), self.reported_clocks.len()) {
            (true, 0) => bail!("Hitrace sched_switch data has no ftrace clock"),
            (_, count) if count > 1 => {
                let clocks = self
                    .reported_clocks
                    .iter()
                    .map(|clock| clock.label)
                    .collect::<Vec<_>>();
                bail!("Hitrace reports conflicting ftrace clocks: {clocks:?}");
            }
            (_, 1) => self.reported_clocks.first().copied(),
            (false, 0) => None,
            _ => unreachable!(),
        };

        if self.switches.is_some() {
            let end_stats = self
                .end_stats
                .as_ref()
                .context("Hitrace sched_switch data has no TRACE_END statistics")?;
            for cpu in &self.detail_cpus {
                if !end_stats.contains(cpu) {
                    bail!("Hitrace TRACE_END statistics are missing CPU {cpu}");
                }
            }
        }

        let mut clock_domains = report.clock_domains;
        if let Some(clock) = ftrace_clock {
            for cpu in self.last_switch.keys() {
                clock_domains
                    .entry(clock.domain(*cpu))
                    .or_insert_with(|| clock.clock_type.to_owned());
            }
        }
        let mut clock_snapshots = report
            .clock_snapshots
            .into_iter()
            .map(|snapshot| ClockSnapshot {
                snapshot_id: snapshot.snapshot_id,
                clock_domain: snapshot.clock_domain,
                clock_value: snapshot.clock_value,
            })
            .collect::<Vec<_>>();
        clock_snapshots.extend(self.clock_snapshots);

        Ok(DecodedLongTermHitrace {
            switches: self.switches,
            clock_snapshots,
            clock_domains,
            ftrace_clock,
            unsupported_plugins: report.unsupported_plugins.into_iter().collect(),
            unsupported_section_types: report.unsupported_section_types.into_iter().collect(),
            unsupported_content: report
                .unsupported_content
                .into_iter()
                .map(|content| UnsupportedHitraceContent {
                    kind: content.kind,
                    value: content.value,
                    byte_offset: content.byte_offset,
                })
                .collect(),
        })
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
}

impl SwitchSpool {
    fn new() -> Result<Self> {
        let file = tempfile::tempfile().context("failed to create bounded sched_switch spool")?;
        Ok(Self {
            writer: ArrowWriter::try_new(file, switch_spool_schema(), None)
                .context("failed to open sched_switch Parquet spool")?,
            rows: Vec::with_capacity(HITRACE_IMPORT_BATCH_ROWS),
        })
    }

    fn push(&mut self, row: SwitchRow) -> Result<()> {
        self.rows.push(row);
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

    fn into_reader(mut self) -> Result<ParquetRecordBatchReader> {
        self.flush_rows()?;
        let mut file = self
            .writer
            .into_inner()
            .context("failed to finish sched_switch Parquet spool")?;
        file.seek(SeekFrom::Start(0))
            .context("failed to rewind sched_switch spool")?;
        ParquetRecordBatchReaderBuilder::try_new(file)
            .context("failed to read sched_switch Parquet spool metadata")?
            .with_batch_size(HITRACE_IMPORT_BATCH_ROWS)
            .build()
            .context("failed to open sched_switch Parquet spool reader")
    }
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

fn write_clock_domains(
    writer: &mut ManagedDatasetWriter,
    domains: &BTreeMap<String, String>,
) -> Result<()> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("clock_domain", DataType::Utf8, false),
        Field::new("clock_type", DataType::Utf8, false),
        Field::new("ticks_per_second", DataType::UInt64, false),
    ]));
    let batch = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![
            Arc::new(StringArray::from_iter_values(domains.keys())),
            Arc::new(StringArray::from_iter_values(domains.values())),
            Arc::new(UInt64Array::from(vec![TICKS_PER_SECOND; domains.len()])),
        ],
    )?;
    let mut table = writer.begin_table("clock_domain", schema)?;
    table.write(&batch)?;
    table.finish()?;
    Ok(())
}

fn write_clock_snapshots(
    writer: &mut ManagedDatasetWriter,
    snapshots: &[ClockSnapshot],
) -> Result<()> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("snapshot_id", DataType::UInt64, false),
        Field::new("clock_domain", DataType::Utf8, false),
        Field::new("clock_value", DataType::UInt64, false),
    ]));
    let mut table = writer.begin_table("clock_snapshot", Arc::clone(&schema))?;
    for rows in snapshots.chunks(HITRACE_IMPORT_BATCH_ROWS) {
        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
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
        )?;
        table.write(&batch)?;
    }
    table.finish()?;
    Ok(())
}

fn write_sched_switches(
    writer: &mut ManagedDatasetWriter,
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
