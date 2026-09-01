use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, anyhow, bail};
use arrow_array::{RecordBatch, StringArray, UInt64Array};
use arrow_schema::{DataType, Field, Schema};

use crate::{
    formats::hitrace,
    protobuf_source::{BufferOptions, native_hook::NativeHookSourceCapture},
    relation_writer::RelationWriter,
};

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
    publication.publish()?;

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

fn ensure_destination_absent(destination: &Path) -> Result<()> {
    match fs::symlink_metadata(destination) {
        Ok(_) => Err(anyhow!(
            "Hitrace decode destination already exists: {}",
            destination.display()
        )),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
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
        ensure_destination_absent(destination)?;
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
        let relations = RelationWriter::new(staging.path());
        Ok(Self {
            destination: destination.to_path_buf(),
            staging: Some(staging),
            relations,
        })
    }

    fn relation_writer(&self) -> RelationWriter {
        self.relations.clone()
    }

    fn publish(mut self) -> Result<()> {
        self.relations
            .validate()
            .context("failed to validate staged Hitrace Parquet relations")?;
        ensure_destination_absent(&self.destination)?;
        let staging = self
            .staging
            .take()
            .expect("unpublished Hitrace decode always owns staging");
        rename_no_replace(staging.path(), &self.destination).with_context(|| {
            format!(
                "failed to publish staged Hitrace relations to {}",
                self.destination.display()
            )
        })?;
        let _published_staging_path = staging.keep();
        Ok(())
    }
}

#[cfg(any(
    target_os = "android",
    target_os = "linux",
    target_os = "redox",
    target_vendor = "apple"
))]
fn rename_no_replace(source: &Path, destination: &Path) -> io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};

    renameat_with(CWD, source, CWD, destination, RenameFlags::NOREPLACE).map_err(io::Error::from)
}

#[cfg(windows)]
fn rename_no_replace(source: &Path, destination: &Path) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

    let source = windows_existing_path(source)?;
    let destination = windows_destination_path(destination)?;
    // Omitting MOVEFILE_REPLACE_EXISTING makes publication fail if another
    // process creates the destination after the initial preflight check.
    // SAFETY: both buffers are NUL-terminated and live for the duration of the
    // call; MoveFileExW does not retain either pointer.
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn windows_existing_path(path: &Path) -> io::Result<Vec<u16>> {
    null_terminated_wide_path(&path.canonicalize()?)
}

#[cfg(windows)]
fn windows_destination_path(path: &Path) -> io::Result<Vec<u16>> {
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "destination has no final path component",
        )
    })?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()?;
    null_terminated_wide_path(&parent.join(name))
}

#[cfg(windows)]
fn null_terminated_wide_path(path: &Path) -> io::Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;

    let mut wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
    if wide.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path contains an interior NUL",
        ));
    }
    wide.push(0);
    Ok(wide)
}

#[cfg(not(any(
    target_os = "android",
    target_os = "linux",
    target_os = "redox",
    target_vendor = "apple",
    windows
)))]
fn rename_no_replace(_source: &Path, _destination: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic no-replace directory publication is unsupported on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::rename_no_replace;

    #[test]
    fn publication_does_not_replace_a_destination_created_after_staging() {
        let parent = tempfile::tempdir().expect("temporary parent is created");
        let staging = parent.path().join("staging");
        let destination = parent.path().join("destination");
        fs::create_dir(&staging).expect("staging directory is created");
        fs::write(staging.join("relation.parquet"), b"staged").expect("staged relation is written");
        fs::create_dir(&destination).expect("racing destination is created");

        rename_no_replace(&staging, &destination)
            .expect_err("publication must not replace a racing destination");

        assert!(
            fs::read_dir(&destination)
                .expect("racing destination remains readable")
                .next()
                .is_none()
        );
        assert!(staging.join("relation.parquet").is_file());
    }
}
