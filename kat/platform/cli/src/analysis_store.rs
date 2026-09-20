use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use miette::{IntoDiagnostic, Result, WrapErr, bail};
use serde::Serialize;

use crate::{
    analysis_record::{AnalysisRecord, Change, Goal, Material},
    run_manifest,
    session_store::{
        SessionLayout, ensure_direct_directory, read_direct_file, resolve_direct_directory,
        validate_direct_file,
    },
};

const DIRECTORY: &str = "analysis";
const RECORD: &str = "record.json";
const LOCK: &str = "write.lock";

#[derive(Serialize)]
pub(super) struct ArchiveReceipt {
    session_id: String,
    previous_revision: u64,
    revision: u64,
    material_id: String,
}

pub(super) fn read(session: &SessionLayout) -> Result<AnalysisRecord> {
    let directory = resolve_direct_directory(session.root(), DIRECTORY)
        .into_diagnostic()
        .wrap_err(
            "Analysis Record is unavailable; initialize it explicitly if it does not exist",
        )?;
    // Windows 上打开旧记录与替换文件可能竞争，读者也在同一稳定锁下读取完整版本。
    let _lock = lock_record(&directory, false)?;
    read_record(session, &directory)
}

fn read_record(session: &SessionLayout, directory: &Path) -> Result<AnalysisRecord> {
    let file = read_direct_file(&directory.join(RECORD), directory, RECORD)
        .into_diagnostic()
        .wrap_err("Analysis Record is missing or inaccessible")?;
    let value: serde_json::Value = serde_json::from_reader(file)
        .into_diagnostic()
        .wrap_err("Analysis Record is corrupt")?;
    if value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(1)
    {
        bail!("unsupported Analysis Record schema_version");
    }
    let record: AnalysisRecord = serde_json::from_value(value)
        .into_diagnostic()
        .wrap_err("Analysis Record is corrupt")?;
    if record.session_id != session.session_id().as_str() {
        bail!("Analysis Record belongs to another Session");
    }
    record.validate().wrap_err("Analysis Record is invalid")?;
    Ok(record)
}

pub(super) fn init(session: &SessionLayout, goal: Goal) -> Result<AnalysisRecord> {
    let record = AnalysisRecord::new(session.session_id().as_str().to_owned(), goal)?;
    let directory = ensure_direct_directory(session.root(), DIRECTORY)
        .into_diagnostic()
        .wrap_err("cannot create Analysis Record directory")?;
    let _lock = lock_record(&directory, true)?;
    match fs::symlink_metadata(directory.join(RECORD)) {
        Ok(_) => bail!("Analysis Record already exists; init never overwrites a record"),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err("cannot inspect Analysis Record");
        }
    }
    persist(&directory, &record)?;
    Ok(record)
}

pub(super) fn update(
    session: &SessionLayout,
    expected_revision: u64,
    changes: Vec<Change>,
) -> Result<AnalysisRecord> {
    let directory = resolve_direct_directory(session.root(), DIRECTORY)
        .into_diagnostic()
        .wrap_err("Analysis Record is unavailable")?;
    let _lock = lock_record(&directory, true)?;
    let mut record = read_record(session, &directory)?;
    if record.revision != expected_revision {
        bail!(
            "Analysis Record revision conflict: expected {}, current {}; read analysis show before retrying",
            expected_revision,
            record.revision
        );
    }
    let runs = run_manifest::resolve_all(session).into_diagnostic()?;
    // 候选只存在当前进程中，整个批次通过校验后才替换已保存记录。
    record.apply(changes, &runs)?;
    advance_revision(&mut record)?;
    persist(&directory, &record)?;
    Ok(record)
}

pub(super) fn append_material(
    session: &SessionLayout,
    material: Material,
) -> Result<ArchiveReceipt> {
    let directory = resolve_direct_directory(session.root(), DIRECTORY)
        .into_diagnostic()
        .wrap_err("Analysis Record is unavailable; initialize it before archiving")?;
    let _lock = lock_record(&directory, true)?;
    let mut record = read_record(session, &directory)?;
    let previous_revision = record.revision;
    let material_id = uuid::Uuid::now_v7().to_string();
    record.materials.insert(material_id.clone(), material);
    record.validate()?;
    advance_revision(&mut record)?;
    persist(&directory, &record)?;
    Ok(ArchiveReceipt {
        session_id: record.session_id,
        previous_revision,
        revision: record.revision,
        material_id,
    })
}

fn advance_revision(record: &mut AnalysisRecord) -> Result<()> {
    record.revision = record
        .revision
        .checked_add(1)
        .ok_or_else(|| miette::miette!("Analysis Record revision exhausted"))?;
    Ok(())
}

fn lock_record(directory: &Path, exclusive: bool) -> Result<File> {
    let path = directory.join(LOCK);
    let file = match OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let checked = validate_direct_file(&path, directory, LOCK).into_diagnostic()?;
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(checked)
                .into_diagnostic()?
        }
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err("cannot open analysis write lock");
        }
    };
    // 锁文件不随 record.json 原子替换，所有写者始终竞争同一个文件。
    if exclusive {
        file.lock()
    } else {
        file.lock_shared()
    }
    .into_diagnostic()
    .wrap_err("cannot lock Analysis Record")?;
    Ok(file)
}

fn persist(directory: &Path, record: &AnalysisRecord) -> Result<()> {
    let destination: PathBuf = directory.join(RECORD);
    if fs::symlink_metadata(&destination).is_ok() {
        validate_direct_file(&destination, directory, RECORD).into_diagnostic()?;
    }
    let mut temporary = tempfile::NamedTempFile::new_in(directory)
        .into_diagnostic()
        .wrap_err("cannot prepare Analysis Record replacement")?;
    serde_json::to_writer(temporary.as_file_mut(), record)
        .into_diagnostic()
        .wrap_err("cannot encode Analysis Record")?;
    temporary.as_file_mut().write_all(b"\n").into_diagnostic()?;
    temporary.as_file_mut().sync_all().into_diagnostic()?;
    temporary
        .persist(&destination)
        .map_err(|error| error.error)
        .into_diagnostic()
        .wrap_err("cannot replace Analysis Record; previous record was retained")?;
    // 提交后响应失败不能回滚；下一次 show 可确认已经保存的 revision。
    Ok(())
}
