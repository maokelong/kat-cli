use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, anyhow, bail};
use arrow_array::{RecordBatch, StringArray, UInt64Array};
use arrow_schema::{DataType, Field, Schema};

use crate::{
    directory_publish::{ensure_destination_absent, publish_directory_no_replace},
    formats::hitrace,
    protobuf_source::{BufferOptions, native_hook::NativeHookSourceCapture},
    relation_writer::RelationWriter,
};

const MATERIALIZATION_VERSION: &str = "hitrace-v1";
const TICKS_PER_SECOND: u64 = 1_000_000_000;

#[derive(Debug)]
pub struct HitraceDecodeReport {
    unsupported_plugins: Vec<String>,
    unsupported_section_types: Vec<u32>,
}

impl HitraceDecodeReport {
    pub fn unsupported_plugins(&self) -> &[String] {
        &self.unsupported_plugins
    }

    pub fn unsupported_section_types(&self) -> &[u32] {
        &self.unsupported_section_types
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{source:#}")]
pub struct HitraceDecodeError {
    #[source]
    source: anyhow::Error,
}

impl HitraceDecodeError {
    fn new(source: anyhow::Error) -> Self {
        Self { source }
    }
}

pub fn decode_hitrace(
    source: impl AsRef<Path>,
    destination: impl AsRef<Path>,
) -> std::result::Result<HitraceDecodeReport, HitraceDecodeError> {
    decode_hitrace_inner(source.as_ref(), destination.as_ref()).map_err(HitraceDecodeError::new)
}

fn decode_hitrace_inner(source: &Path, destination: &Path) -> Result<HitraceDecodeReport> {
    decode_hitrace_inner_with_publish_gate(source, destination, |_, _| Ok(()))
}

fn decode_hitrace_inner_with_publish_gate(
    source: &Path,
    destination: &Path,
    publish_gate: impl FnOnce(&Path, &Path) -> Result<()>,
) -> Result<HitraceDecodeReport> {
    let publication = HitracePublication::stage(destination)?;
    let relations = publication.relation_writer();
    let mut capture = NativeHookSourceCapture::new(BufferOptions::default(), relations.clone())
        .context("failed to initialize profiler descriptor relation capture")?;
    let report = hitrace::decode_file(source, &mut capture)
        .with_context(|| format!("failed to decode hitrace file: {}", source.display()))?;
    capture
        .finish()
        .context("failed to finish profiler descriptor relations")?;
    write_clock_relations(&relations, &report)?;
    publication.publish(publish_gate)?;

    Ok(HitraceDecodeReport {
        unsupported_plugins: report.unsupported_plugins.into_iter().collect(),
        unsupported_section_types: report.unsupported_section_types.into_iter().collect(),
    })
}

fn write_clock_relations(
    relations: &RelationWriter,
    report: &hitrace::HitraceDecodeReport,
) -> Result<()> {
    let domain_schema = Arc::new(Schema::new(vec![
        Field::new("clock_domain", DataType::Utf8, false),
        Field::new("clock_type", DataType::Utf8, false),
        Field::new("ticks_per_second", DataType::UInt64, false),
    ]));
    let domain_batch = RecordBatch::try_new(
        Arc::clone(&domain_schema),
        vec![
            Arc::new(StringArray::from_iter_values(report.clock_domains.keys())),
            Arc::new(StringArray::from_iter_values(report.clock_domains.values())),
            Arc::new(UInt64Array::from(vec![
                TICKS_PER_SECOND;
                report.clock_domains.len()
            ])),
        ],
    )
    .context("failed to build clock_domain relation")?;
    let mut domains = relations.begin("clock_domain", domain_schema)?;
    domains.write(&domain_batch)?;
    domains.finish()?;

    let snapshot_schema = Arc::new(Schema::new(vec![
        Field::new("snapshot_id", DataType::UInt64, false),
        Field::new("clock_domain", DataType::Utf8, false),
        Field::new("clock_value", DataType::UInt64, false),
    ]));
    let snapshot_batch = RecordBatch::try_new(
        Arc::clone(&snapshot_schema),
        vec![
            Arc::new(UInt64Array::from_iter_values(
                report
                    .clock_snapshots
                    .iter()
                    .map(|snapshot| snapshot.snapshot_id),
            )),
            Arc::new(StringArray::from_iter_values(
                report
                    .clock_snapshots
                    .iter()
                    .map(|snapshot| snapshot.clock_domain.as_str()),
            )),
            Arc::new(UInt64Array::from_iter_values(
                report
                    .clock_snapshots
                    .iter()
                    .map(|snapshot| snapshot.clock_value),
            )),
        ],
    )
    .context("failed to build clock_snapshot relation")?;
    let mut snapshots = relations.begin("clock_snapshot", snapshot_schema)?;
    snapshots.write(&snapshot_batch)?;
    snapshots.finish()?;
    Ok(())
}

fn ensure_hitrace_destination_absent(destination: &Path) -> Result<()> {
    match ensure_destination_absent(destination) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => Err(anyhow!(
            "Hitrace decode destination already exists: {}",
            destination.display()
        )),
        Err(source) => Err(source).map_err(|source| {
            anyhow!(
                "failed to inspect Hitrace decode destination {}: {source}",
                destination.display()
            )
        }),
    }
}

struct HitracePublication {
    destination: PathBuf,
    staging: Option<tempfile::TempDir>,
    relations: RelationWriter,
}

impl HitracePublication {
    fn stage(destination: &Path) -> Result<Self> {
        ensure_hitrace_destination_absent(destination)?;
        let parent = destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let parent_metadata = fs::metadata(parent).with_context(|| {
            format!(
                "failed to inspect Hitrace decode destination parent {}",
                parent.display()
            )
        })?;
        if !parent_metadata.is_dir() {
            bail!(
                "Hitrace decode destination parent is not a directory: {}",
                parent.display()
            );
        }
        let staging = tempfile::Builder::new()
            .prefix(".kat-datasource-staging-")
            .tempdir_in(parent)
            .with_context(|| {
                format!(
                    "failed to create private Hitrace decode staging beside {}",
                    destination.display()
                )
            })?;
        let relations = RelationWriter::new(staging.path(), MATERIALIZATION_VERSION);
        Ok(Self {
            destination: destination.to_path_buf(),
            staging: Some(staging),
            relations,
        })
    }

    fn relation_writer(&self) -> RelationWriter {
        self.relations.clone()
    }

    fn publish(mut self, publish_gate: impl FnOnce(&Path, &Path) -> Result<()>) -> Result<()> {
        self.relations
            .validate()
            .context("failed to validate staged Hitrace Parquet relations")?;
        ensure_hitrace_destination_absent(&self.destination)?;
        let staging_path = self
            .staging
            .as_ref()
            .expect("unpublished Hitrace decode always owns staging")
            .path();
        publish_gate(staging_path, &self.destination)?;
        let staging = self
            .staging
            .take()
            .expect("unpublished Hitrace decode always owns staging");
        publish_directory_no_replace(staging.path(), &self.destination).with_context(|| {
            format!(
                "failed to publish staged Hitrace relations to {}",
                self.destination.display()
            )
        })?;
        let _published_staging_path = staging.keep();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env,
        fs::{self, File},
        path::{Path, PathBuf},
        process::{Child, Command, Output, Stdio},
        thread,
        time::{Duration, Instant},
    };

    use arrow_array::{StringArray, UInt64Array};
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

    use super::decode_hitrace_inner_with_publish_gate;

    const HEADER_SIZE: usize = 1024;
    const HEADER_MAGIC: u64 = 0x464F_5250_534F_484F;
    const RACE_PUBLISHER_ENV: &str = "KAT_DATASOURCE_HITRACE_RACE_PUBLISHER";
    const RACE_ROOT_ENV: &str = "KAT_DATASOURCE_HITRACE_RACE_ROOT";
    const STAGING_PREFIX: &str = ".kat-datasource-staging-";

    #[cfg(any(
        target_os = "android",
        target_os = "linux",
        target_os = "redox",
        target_vendor = "apple",
        windows
    ))]
    #[test]
    fn simultaneous_full_decode_processes_publish_exactly_one_complete_candidate() {
        let root = tempfile::tempdir().expect("race root is created");
        let destination = root.path().join("destination");
        let clock_values = [("first", 111_111_u64), ("second", 222_222_u64)];
        for &(publisher, clock_value) in &clock_values {
            fs::write(
                root.path().join(format!("source-{publisher}.htrace")),
                hitrace_header(clock_value),
            )
            .unwrap_or_else(|error| panic!("failed to write {publisher} source: {error}"));
        }

        let executable = env::current_exe().expect("current test executable is available");
        let children = ["first", "second"].map(|publisher| {
            let child = Command::new(&executable)
                .args([
                    "--exact",
                    "hitrace_decode::tests::full_decode_publish_race_child",
                    "--nocapture",
                ])
                .env(RACE_ROOT_ENV, root.path())
                .env(RACE_PUBLISHER_ENV, publisher)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap_or_else(|error| panic!("failed to spawn {publisher} publisher: {error}"));
            ChildGuard::new(child)
        });

        let ready = wait_until(Duration::from_secs(30), || {
            ["first", "second"]
                .into_iter()
                .all(|publisher| root.path().join(format!("ready-{publisher}")).is_file())
        });
        if !ready {
            let _ = fs::write(root.path().join("go"), []);
            let outputs = children.map(wait_for_child);
            panic!("full-decode publishers did not reach the publish gate: {outputs:#?}");
        }

        assert!(
            !destination.exists(),
            "destination must remain absent while both complete candidates wait"
        );
        let staging = staging_directories(root.path());
        assert_eq!(
            staging.len(),
            2,
            "both publishers must own one staging directory"
        );
        let mut candidate_values = staging
            .iter()
            .map(|candidate| {
                assert_exact_clock_relations(candidate);
                boottime_clock_value(candidate)
            })
            .collect::<Vec<_>>();
        candidate_values.sort_unstable();
        assert_eq!(candidate_values, [111_111, 222_222]);

        fs::write(root.path().join("go"), []).expect("publish gate is released");
        for output in children.map(wait_for_child) {
            assert!(
                output.status.success(),
                "publisher failed\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let outcomes = ["first", "second"].map(|publisher| {
            let outcome = fs::read_to_string(root.path().join(format!("outcome-{publisher}")))
                .unwrap_or_else(|error| panic!("missing {publisher} outcome: {error}"));
            (publisher, outcome)
        });
        let winner = outcomes
            .iter()
            .filter_map(|(publisher, outcome)| (outcome == "won").then_some(*publisher))
            .collect::<Vec<_>>();
        assert_eq!(
            winner.len(),
            1,
            "exactly one publisher must win: {outcomes:?}"
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|(_, outcome)| outcome == "lost")
                .count(),
            1,
            "exactly one publisher must lose: {outcomes:?}"
        );
        let winning_clock_value = clock_values
            .iter()
            .find_map(|(publisher, value)| (*publisher == winner[0]).then_some(*value))
            .expect("winner has one source clock value");
        assert_exact_clock_relations(&destination);
        assert_eq!(boottime_clock_value(&destination), winning_clock_value);
        assert!(
            staging_directories(root.path()).is_empty(),
            "winner and loser staging directories must both be consumed or cleaned"
        );
    }

    #[test]
    fn full_decode_publish_race_child() {
        let Some(root) = env::var_os(RACE_ROOT_ENV).map(PathBuf::from) else {
            return;
        };
        let publisher = env::var(RACE_PUBLISHER_ENV).expect("publisher identity is set");
        let source = root.join(format!("source-{publisher}.htrace"));
        let destination = root.join("destination");

        let result = decode_hitrace_inner_with_publish_gate(
            &source,
            &destination,
            |staging, selected_destination| {
                assert_eq!(selected_destination, destination);
                assert_exact_clock_relations(staging);
                fs::write(root.join(format!("ready-{publisher}")), [])?;
                if !wait_until(Duration::from_secs(60), || root.join("go").is_file()) {
                    anyhow::bail!("publisher timed out at the final publish gate");
                }
                Ok(())
            },
        );
        let outcome = match result {
            Ok(_) => "won",
            Err(error) => {
                assert!(
                    format!("{error:#}").contains("failed to publish staged Hitrace relations"),
                    "unexpected full-decode failure: {error:#}"
                );
                let conflict = error
                    .chain()
                    .find_map(|source| source.downcast_ref::<std::io::Error>())
                    .expect("publish failure retains its I/O cause");
                assert_eq!(conflict.kind(), std::io::ErrorKind::AlreadyExists);
                "lost"
            }
        };
        fs::write(root.join(format!("outcome-{publisher}")), outcome)
            .expect("publisher outcome is written");
    }

    fn hitrace_header(boottime_clock_value: u64) -> Vec<u8> {
        let mut bytes = vec![0_u8; HEADER_SIZE];
        bytes[0..8].copy_from_slice(&HEADER_MAGIC.to_le_bytes());
        bytes[8..16].copy_from_slice(&(HEADER_SIZE as u64).to_le_bytes());
        bytes[56..60].copy_from_slice(&1_000_u32.to_le_bytes());
        bytes[60..68].copy_from_slice(&boottime_clock_value.to_le_bytes());
        bytes
    }

    fn staging_directories(parent: &Path) -> Vec<PathBuf> {
        let mut staging = fs::read_dir(parent)
            .expect("materialization parent can be listed")
            .filter_map(|entry| {
                let entry = entry.expect("materialization entry can be read");
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(STAGING_PREFIX)
                    .then(|| entry.path())
            })
            .collect::<Vec<_>>();
        staging.sort();
        staging
    }

    fn assert_exact_clock_relations(root: &Path) {
        let mut entries = fs::read_dir(root)
            .expect("candidate materialization can be listed")
            .map(|entry| {
                let entry = entry.expect("candidate relation can be read");
                assert!(
                    entry
                        .file_type()
                        .expect("relation type is available")
                        .is_file(),
                    "candidate relation must be an ordinary file"
                );
                entry
                    .file_name()
                    .into_string()
                    .expect("relation name is Unicode")
            })
            .collect::<Vec<_>>();
        entries.sort();
        assert_eq!(entries, ["clock_domain.parquet", "clock_snapshot.parquet"]);
    }

    fn boottime_clock_value(root: &Path) -> u64 {
        let reader = ParquetRecordBatchReaderBuilder::try_new(
            File::open(root.join("clock_snapshot.parquet"))
                .expect("clock snapshot relation can be opened"),
        )
        .expect("clock snapshot metadata is valid")
        .build()
        .expect("clock snapshot reader is created");
        for batch in reader {
            let batch = batch.expect("clock snapshot batch is valid");
            let domains = batch
                .column(1)
                .as_any()
                .downcast_ref::<StringArray>()
                .expect("clock_domain is a string column");
            let values = batch
                .column(2)
                .as_any()
                .downcast_ref::<UInt64Array>()
                .expect("clock_value is a uint64 column");
            for index in 0..batch.num_rows() {
                if domains.value(index) == "boottime" {
                    return values.value(index);
                }
            }
        }
        panic!("clock snapshot relation has no boottime row")
    }

    fn wait_until(timeout: Duration, predicate: impl Fn() -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if predicate() {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        predicate()
    }

    struct ChildGuard(Option<Child>);

    impl ChildGuard {
        fn new(child: Child) -> Self {
            Self(Some(child))
        }

        fn child_mut(&mut self) -> &mut Child {
            self.0.as_mut().expect("child guard owns one process")
        }

        fn take(&mut self) -> Child {
            self.0.take().expect("child guard owns one process")
        }
    }

    impl Drop for ChildGuard {
        fn drop(&mut self) {
            if let Some(child) = self.0.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    fn wait_for_child(mut child: ChildGuard) -> Output {
        let deadline = Instant::now() + Duration::from_secs(65);
        loop {
            match child.child_mut().try_wait() {
                Ok(Some(_)) => {
                    return child
                        .take()
                        .wait_with_output()
                        .expect("publisher output is collected");
                }
                Ok(None) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(10));
                }
                Ok(None) => {
                    let mut process = child.take();
                    let _ = process.kill();
                    let output = process
                        .wait_with_output()
                        .expect("timed-out publisher is reaped");
                    panic!("publisher did not exit after the publish gate: {output:#?}");
                }
                Err(error) => panic!("failed to inspect publisher status: {error}"),
            }
        }
    }
}
