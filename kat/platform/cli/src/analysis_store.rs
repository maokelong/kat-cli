use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use miette::{IntoDiagnostic, Result, WrapErr, bail};
use serde::Serialize;

use crate::{
    analysis_record::{AnalysisRecord, Material, ReportNode, SCHEMA_VERSION, SaveInput},
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
    require_record(session)?;
    let directory = resolve_direct_directory(session.root(), DIRECTORY)
        .into_diagnostic()
        .wrap_err("Analysis Record is inaccessible")?;
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
        != Some(u64::from(SCHEMA_VERSION))
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

pub(super) fn save(
    session: &SessionLayout,
    expected_revision: u64,
    input: SaveInput,
) -> Result<(AnalysisRecord, Vec<ReportNode>)> {
    let directory = if expected_revision == 0 {
        ensure_direct_directory(session.root(), DIRECTORY)
            .into_diagnostic()
            .wrap_err("cannot create Analysis Record directory")?
    } else {
        require_record(session)?;
        resolve_direct_directory(session.root(), DIRECTORY)
            .into_diagnostic()
            .wrap_err("Analysis Record is inaccessible")?
    };
    let _lock = lock_record(&directory, true)?;
    let exists = record_exists(&directory.join(RECORD))?;
    let mut record = if exists {
        read_record(session, &directory)?
    } else {
        if expected_revision != 0 {
            bail!("Analysis Record does not exist; first save requires --expected-revision 0");
        }
        let goal = input
            .goal
            .clone()
            .ok_or_else(|| miette::miette!("First analysis save must include the analysis goal"))?;
        AnalysisRecord::new(session.session_id().as_str().to_owned(), goal)?
    };
    if exists && record.revision != expected_revision {
        bail!(
            "Analysis Record revision conflict: expected {}, current {}; read analysis show before retrying",
            expected_revision,
            record.revision
        );
    }
    let runs = run_manifest::resolve_all(session).into_diagnostic()?;
    // 候选只存在当前进程中，整个批次通过校验后才替换已保存记录。
    let tree = record.apply(input, &runs)?;
    if exists {
        advance_revision(&mut record)?;
    }
    persist(&directory, &record)?;
    // 回执复用提交前校验的树，不再扫描可能已发生变化的 Session。
    Ok((record, tree))
}

pub(super) fn append_material(
    session: &SessionLayout,
    material: Material,
) -> Result<ArchiveReceipt> {
    let directory = resolve_direct_directory(session.root(), DIRECTORY)
        .into_diagnostic()
        .wrap_err("Analysis Record is unavailable; save its goal before archiving")?;
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

fn require_record(session: &SessionLayout) -> Result<()> {
    let directory = session.root().join(DIRECTORY);
    if !record_exists(&directory)? {
        bail!(
            "Analysis Record does not exist; first save requires --expected-revision 0 and a goal"
        );
    }
    // 先验证目录，避免把错误布局中的记录误报成普通缺失。
    let directory = resolve_direct_directory(session.root(), DIRECTORY)
        .into_diagnostic()
        .wrap_err("Analysis Record is inaccessible")?;
    if !record_exists(&directory.join(RECORD))? {
        bail!(
            "Analysis Record does not exist; first save requires --expected-revision 0 and a goal"
        );
    }
    Ok(())
}

fn record_exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error)
            .into_diagnostic()
            .wrap_err("cannot inspect Analysis Record"),
    }
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
