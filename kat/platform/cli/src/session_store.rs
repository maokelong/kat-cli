use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use miette::Diagnostic;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const SESSIONS_DIRECTORY: &str = "sessions";
const LEASES_DIRECTORY: &str = ".leases";
const DELETIONS_DIRECTORY: &str = ".deletions";
const SESSION_MARKER: &str = "session.json";
const MATERIALIZATIONS_DIRECTORY: &str = "materializations";
const SCRATCH_DIRECTORY: &str = "scratch";
const RUNS_DIRECTORY: &str = "runs";

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct SessionId(String);

impl SessionId {
    pub(super) fn generate() -> Self {
        Self(uuid::Uuid::now_v7().to_string())
    }

    pub(super) fn parse(value: &str) -> Option<Self> {
        canonical_uuid_v7(value).then(|| Self(value.to_owned()))
    }

    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct RunId(String);

impl RunId {
    pub(super) fn generate() -> Self {
        Self(uuid::Uuid::now_v7().to_string())
    }

    pub(super) fn parse(value: &str) -> Option<Self> {
        canonical_uuid_v7(value).then(|| Self(value.to_owned()))
    }

    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

fn canonical_uuid_v7(value: &str) -> bool {
    uuid::Uuid::parse_str(value)
        .ok()
        .is_some_and(|identity| identity.get_version_num() == 7 && identity.to_string() == value)
}

pub(super) struct SessionStore {
    data_home: PathBuf,
}

impl SessionStore {
    pub(super) fn new(data_home: &Path) -> Self {
        Self {
            data_home: data_home.to_path_buf(),
        }
    }

    pub(super) fn create_run(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> Result<RunAllocation, SessionStoreError> {
        let roots = self.create_storage_roots()?;
        let lease_path = roots.leases.join(format!("{}.lock", session_id.as_str()));
        let lease_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&lease_path)
            .map_err(|source| SessionStoreError::CreateLease { source })?;
        let lease = match SessionLease::try_shared(lease_file) {
            Ok(lease) => lease,
            Err(error) => {
                drop(error);
                let _ = fs::remove_file(&lease_path);
                return Err(SessionStoreError::CreateLeaseLock);
            }
        };
        let session_root = roots.sessions.join(session_id.as_str());
        if let Err(source) = fs::create_dir(&session_root) {
            drop(lease);
            let _ = fs::remove_file(&lease_path);
            return Err(SessionStoreError::CreateSession { source });
        }
        let created = (|| {
            let session_root =
                canonical_direct_directory(&session_root, &roots.sessions, session_id.as_str())
                    .map_err(SessionStoreError::PrepareSessionLayout)?;
            let materializations =
                create_direct_directory(&session_root, MATERIALIZATIONS_DIRECTORY)
                    .map_err(SessionStoreError::PrepareSessionLayout)?;
            let scratch = create_direct_directory(&session_root, SCRATCH_DIRECTORY)
                .map_err(SessionStoreError::PrepareSessionLayout)?;
            let runs = create_direct_directory(&session_root, RUNS_DIRECTORY)
                .map_err(SessionStoreError::PrepareSessionLayout)?;
            Ok(SessionLayout {
                session_id,
                root: session_root,
                materializations,
                scratch,
                runs,
            })
        })();
        let layout = match created {
            Ok(layout) => layout,
            Err(error) => {
                drop(lease);
                let _ = remove_exact_entry(&session_root);
                let _ = fs::remove_file(&lease_path);
                return Err(error);
            }
        };
        match RunAllocation::create(layout, run_id, lease, lease_path, false) {
            Ok(allocation) => Ok(allocation),
            Err(error) => {
                drop(error.lease);
                let _ = remove_exact_entry(&session_root);
                let _ = fs::remove_file(
                    roots
                        .leases
                        .join(format!("{}.lock", error.session_id.as_str())),
                );
                Err(error.error)
            }
        }
    }

    pub(super) fn open(&self, session: &str) -> Result<OpenedSession, SessionStoreError> {
        let session_id = SessionId::parse(session).ok_or_else(|| SessionStoreError::NotFound {
            session_id: diagnostic_safe_argument(session),
        })?;
        let roots = self.resolve_storage_roots(&session_id)?;
        let lease_path = roots.leases.join(format!("{}.lock", session_id.as_str()));
        let lease_file = open_lease_file(
            &lease_path,
            &roots.leases,
            &format!("{}.lock", session_id.as_str()),
        )
        .map_err(|error| match error {
            DirectPathError::Missing(_) => {
                match path_exists(&roots.sessions.join(session_id.as_str())) {
                    Ok(false) => SessionStoreError::NotFound {
                        session_id: session_id.as_str().to_owned(),
                    },
                    Ok(true) | Err(_) => SessionStoreError::Corrupted,
                }
            }
            other => SessionStoreError::ResolveLease(other),
        })?;
        let lease = SessionLease::try_shared(lease_file).map_err(|error| {
            if error.kind() == io::ErrorKind::WouldBlock {
                SessionStoreError::Unavailable {
                    session_id: session_id.as_str().to_owned(),
                }
            } else {
                SessionStoreError::LockLease(error)
            }
        })?;
        let layout = validate_session_layout(&roots.sessions, session_id)?;
        Ok(OpenedSession { layout, lease })
    }

    pub(super) fn create_run_in(
        &self,
        session: &str,
        run_id: RunId,
    ) -> Result<RunAllocation, ExistingRunAllocationError> {
        let OpenedSession { layout, lease } = self.open(session)?;
        let lease_path = layout
            .root
            .parent()
            .expect("validated Session has sessions parent")
            .join(LEASES_DIRECTORY)
            .join(format!("{}.lock", layout.session_id.as_str()));
        RunAllocation::create(layout, run_id, lease, lease_path, true).map_err(|error| {
            ExistingRunAllocationError {
                error: error.error,
                lease: Some(error.lease),
            }
        })
    }

    pub(super) fn delete(&self, session: &str) -> Result<SessionId, SessionStoreError> {
        let session_id = SessionId::parse(session).ok_or_else(|| SessionStoreError::NotFound {
            session_id: diagnostic_safe_argument(session),
        })?;
        let roots = self.resolve_storage_roots(&session_id)?;
        let session_path = roots.sessions.join(session_id.as_str());
        let tombstone = roots.deletions.join(session_id.as_str());
        let lease_path = roots.leases.join(format!("{}.lock", session_id.as_str()));
        let lease_file = match open_lease_file(
            &lease_path,
            &roots.leases,
            &format!("{}.lock", session_id.as_str()),
        ) {
            Ok(file) => file,
            Err(DirectPathError::Missing(_))
                if !path_exists(&session_path)? && !path_exists(&tombstone)? =>
            {
                return Err(SessionStoreError::NotFound {
                    session_id: session_id.as_str().to_owned(),
                });
            }
            Err(DirectPathError::Missing(_)) => return Err(SessionStoreError::Corrupted),
            Err(error) => return Err(SessionStoreError::ResolveLease(error)),
        };
        if let Err(error) = lease_file.try_lock() {
            let error = io::Error::from(error);
            return if error.kind() == io::ErrorKind::WouldBlock {
                Err(SessionStoreError::InUse {
                    session_id: session_id.as_str().to_owned(),
                })
            } else {
                Err(SessionStoreError::LockLease(error))
            };
        }

        let session_exists = path_exists(&session_path)?;
        let tombstone_exists = path_exists(&tombstone)?;
        match (session_exists, tombstone_exists) {
            (false, false) => Err(SessionStoreError::NotFound {
                session_id: session_id.as_str().to_owned(),
            }),
            (true, true) => Err(SessionStoreError::InvalidDeletionState),
            (true, false) => {
                validate_session_layout(&roots.sessions, session_id.clone())?;
                rename_no_replace(&session_path, &tombstone)
                    .map_err(SessionStoreError::MoveToTombstone)?;
                remove_tombstone(&roots.deletions, &session_id)?;
                Ok(session_id)
            }
            (false, true) => {
                remove_tombstone(&roots.deletions, &session_id)?;
                Ok(session_id)
            }
        }
    }

    fn create_storage_roots(&self) -> Result<StorageRoots, SessionStoreError> {
        let data_home =
            dunce::canonicalize(&self.data_home).map_err(SessionStoreError::ResolveDataHome)?;
        let sessions = ensure_direct_directory(&data_home, SESSIONS_DIRECTORY)
            .map_err(SessionStoreError::PrepareStorage)?;
        let leases = ensure_direct_directory(&sessions, LEASES_DIRECTORY)
            .map_err(SessionStoreError::PrepareStorage)?;
        let deletions = ensure_direct_directory(&sessions, DELETIONS_DIRECTORY)
            .map_err(SessionStoreError::PrepareStorage)?;
        Ok(StorageRoots {
            sessions,
            leases,
            deletions,
        })
    }

    fn resolve_storage_roots(
        &self,
        session_id: &SessionId,
    ) -> Result<StorageRoots, SessionStoreError> {
        let data_home = match dunce::canonicalize(&self.data_home) {
            Ok(path) => path,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(SessionStoreError::NotFound {
                    session_id: session_id.as_str().to_owned(),
                });
            }
            Err(error) => return Err(SessionStoreError::ResolveDataHome(error)),
        };
        let sessions = resolve_direct_directory(&data_home, SESSIONS_DIRECTORY).map_err(
            |error| match error {
                DirectPathError::Missing(_) => SessionStoreError::NotFound {
                    session_id: session_id.as_str().to_owned(),
                },
                other => SessionStoreError::PrepareStorage(other),
            },
        )?;
        let leases = resolve_direct_directory(&sessions, LEASES_DIRECTORY)
            .map_err(SessionStoreError::PrepareStorage)?;
        let deletions = resolve_direct_directory(&sessions, DELETIONS_DIRECTORY)
            .map_err(SessionStoreError::PrepareStorage)?;
        Ok(StorageRoots {
            sessions,
            leases,
            deletions,
        })
    }
}

struct StorageRoots {
    sessions: PathBuf,
    leases: PathBuf,
    deletions: PathBuf,
}

pub(super) struct SessionLayout {
    session_id: SessionId,
    root: PathBuf,
    materializations: PathBuf,
    scratch: PathBuf,
    runs: PathBuf,
}

impl SessionLayout {
    pub(super) fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    pub(super) fn materializations(&self) -> &Path {
        &self.materializations
    }

    pub(super) fn runs(&self) -> &Path {
        &self.runs
    }
}

pub(super) struct OpenedSession {
    layout: SessionLayout,
    lease: SessionLease,
}

impl OpenedSession {
    pub(super) fn layout(&self) -> &SessionLayout {
        &self.layout
    }

    pub(super) fn into_lease(self) -> SessionLease {
        self.lease
    }
}

pub(super) struct SessionLease {
    file: File,
}

impl SessionLease {
    pub(super) fn try_shared(file: File) -> io::Result<Self> {
        file.try_lock_shared()?;
        Ok(Self { file })
    }
}

impl Drop for SessionLease {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

pub(super) struct RunAllocation {
    layout: SessionLayout,
    run_id: RunId,
    candidate: PathBuf,
    scratch: PathBuf,
    lease: Option<SessionLease>,
    lease_path: PathBuf,
    session_published: bool,
    run_published: bool,
}

struct RunAllocationCreationError {
    session_id: SessionId,
    error: SessionStoreError,
    lease: SessionLease,
}

pub(super) struct ExistingRunAllocationError {
    pub(super) error: SessionStoreError,
    pub(super) lease: Option<SessionLease>,
}

impl From<SessionStoreError> for ExistingRunAllocationError {
    fn from(error: SessionStoreError) -> Self {
        Self { error, lease: None }
    }
}

impl RunAllocation {
    fn create(
        layout: SessionLayout,
        run_id: RunId,
        lease: SessionLease,
        lease_path: PathBuf,
        session_published: bool,
    ) -> Result<Self, RunAllocationCreationError> {
        let candidate = layout.runs.join(run_id.as_str());
        if let Err(source) = fs::create_dir(&candidate) {
            return Err(RunAllocationCreationError {
                session_id: layout.session_id.clone(),
                error: SessionStoreError::CreateRunCandidate(source),
                lease,
            });
        }
        let candidate = match canonical_direct_directory(&candidate, &layout.runs, run_id.as_str())
        {
            Ok(path) => path,
            Err(error) => {
                let _ = remove_exact_entry(&candidate);
                return Err(RunAllocationCreationError {
                    session_id: layout.session_id.clone(),
                    error: SessionStoreError::PrepareRunCandidate(error),
                    lease,
                });
            }
        };
        let scratch_path = layout.scratch.join(run_id.as_str());
        if let Err(error) = fs::create_dir(&scratch_path) {
            let _ = remove_private_run_entry(&layout, RUNS_DIRECTORY, run_id.as_str());
            return Err(RunAllocationCreationError {
                session_id: layout.session_id.clone(),
                error: SessionStoreError::PrepareRunCandidate(DirectPathError::Io(error)),
                lease,
            });
        }
        let scratch = match validate_created_scratch(&layout, &scratch_path, run_id.as_str()) {
            Ok(path) => path,
            Err(error) => {
                let _ = remove_private_run_entry(&layout, RUNS_DIRECTORY, run_id.as_str());
                return Err(RunAllocationCreationError {
                    session_id: layout.session_id.clone(),
                    error: SessionStoreError::PrepareRunCandidate(error),
                    lease,
                });
            }
        };
        Ok(Self {
            layout,
            run_id,
            candidate,
            scratch,
            lease: Some(lease),
            lease_path,
            session_published,
            run_published: false,
        })
    }

    pub(super) fn layout(&self) -> &SessionLayout {
        &self.layout
    }

    pub(super) fn run_id(&self) -> &RunId {
        &self.run_id
    }

    pub(super) fn candidate(&self) -> &Path {
        &self.candidate
    }

    pub(super) fn scratch(&self) -> &Path {
        &self.scratch
    }

    pub(super) fn clean_scratch(&self) -> Result<(), SessionStoreError> {
        remove_private_run_entry(&self.layout, SCRATCH_DIRECTORY, self.run_id.as_str())
            .map_err(SessionStoreError::CleanScratch)
    }

    pub(super) fn mark_run_published(&mut self) {
        self.run_published = true;
    }

    pub(super) fn publish_session(&mut self) -> Result<(), SessionStoreError> {
        if self.session_published {
            return Ok(());
        }
        publish_session_marker(&self.layout)?;
        self.session_published = true;
        Ok(())
    }

    pub(super) fn session_is_published(&self) -> bool {
        self.session_published
    }

    pub(super) fn into_lease(mut self) -> SessionLease {
        self.lease
            .take()
            .expect("Run allocation owns one Session lease")
    }
}

impl Drop for RunAllocation {
    fn drop(&mut self) {
        if !self.session_published {
            let _ = remove_session_root(&self.layout);
            drop(self.lease.take());
            let _ = remove_lease_file(&self.layout, &self.lease_path);
        } else if !self.run_published {
            let _ = remove_private_run_entry(&self.layout, SCRATCH_DIRECTORY, self.run_id.as_str());
            let _ = remove_private_run_entry(&self.layout, RUNS_DIRECTORY, self.run_id.as_str());
        }
    }
}

fn validate_created_scratch(
    layout: &SessionLayout,
    scratch: &Path,
    run_id: &str,
) -> Result<PathBuf, DirectPathError> {
    match canonical_direct_directory(scratch, &layout.scratch, run_id) {
        Ok(path) => Ok(path),
        Err(error) => {
            let _ = remove_private_run_entry(layout, SCRATCH_DIRECTORY, run_id);
            Err(error)
        }
    }
}

fn remove_private_run_entry(
    layout: &SessionLayout,
    parent_name: &str,
    entry_name: &str,
) -> Result<(), DirectPathError> {
    let sessions = layout.root.parent().ok_or(DirectPathError::Invalid)?;
    let root = canonical_direct_directory(&layout.root, sessions, layout.session_id.as_str())?;
    let parent = canonical_direct_directory(&root.join(parent_name), &root, parent_name)?;
    remove_exact_entry(&parent.join(entry_name)).map_err(DirectPathError::Io)
}

fn remove_session_root(layout: &SessionLayout) -> Result<(), DirectPathError> {
    let sessions = layout.root.parent().ok_or(DirectPathError::Invalid)?;
    canonical_direct_directory(&layout.root, sessions, layout.session_id.as_str())?;
    remove_exact_entry(&layout.root).map_err(DirectPathError::Io)
}

fn remove_lease_file(layout: &SessionLayout, lease_path: &Path) -> Result<(), DirectPathError> {
    let sessions = layout.root.parent().ok_or(DirectPathError::Invalid)?;
    let leases =
        canonical_direct_directory(&sessions.join(LEASES_DIRECTORY), sessions, LEASES_DIRECTORY)?;
    let name = format!("{}.lock", layout.session_id.as_str());
    validate_direct_file(lease_path, &leases, &name)?;
    remove_exact_entry(lease_path).map_err(DirectPathError::Io)
}

/// Removes one exact entry without following a Runtime-created link or junction.
fn remove_exact_entry(path: &Path) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata_is_directory_reparse_point(&metadata) {
        fs::remove_dir(path)
    } else if metadata.file_type().is_symlink() || metadata_is_reparse_point(&metadata) {
        fs::remove_file(path)
    } else if metadata.file_type().is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionMarker {
    session_id: String,
}

fn publish_session_marker(layout: &SessionLayout) -> Result<(), SessionStoreError> {
    let destination = layout.root.join(SESSION_MARKER);
    if fs::symlink_metadata(&destination).is_ok() {
        return Err(SessionStoreError::PrematureSessionMarker);
    }
    let mut temporary = tempfile::NamedTempFile::new_in(&layout.root)
        .map_err(SessionStoreError::CreateSessionMarker)?;
    serde_json::to_writer(
        temporary.as_file_mut(),
        &SessionMarker {
            session_id: layout.session_id.as_str().to_owned(),
        },
    )
    .map_err(SessionStoreError::EncodeSessionMarker)?;
    temporary
        .as_file_mut()
        .write_all(b"\n")
        .map_err(SessionStoreError::WriteSessionMarker)?;
    temporary
        .as_file_mut()
        .sync_all()
        .map_err(SessionStoreError::FlushSessionMarker)?;
    temporary
        .persist_noclobber(&destination)
        .map_err(|error| SessionStoreError::PublishSessionMarker(error.error))?;
    Ok(())
}

fn validate_session_layout(
    sessions: &Path,
    session_id: SessionId,
) -> Result<SessionLayout, SessionStoreError> {
    let root = canonical_direct_directory(
        &sessions.join(session_id.as_str()),
        sessions,
        session_id.as_str(),
    )
    .map_err(|error| match error {
        DirectPathError::Missing(_) => SessionStoreError::NotFound {
            session_id: session_id.as_str().to_owned(),
        },
        other => SessionStoreError::InvalidSessionLayout(other),
    })?;
    let marker = read_direct_file(&root.join(SESSION_MARKER), &root, SESSION_MARKER)
        .map_err(SessionStoreError::InvalidSessionLayout)?;
    let marker: SessionMarker =
        serde_json::from_reader(marker).map_err(SessionStoreError::DecodeSessionMarker)?;
    if marker.session_id != session_id.as_str() {
        return Err(SessionStoreError::InvalidSessionIdentity);
    }
    let materializations = resolve_direct_directory(&root, MATERIALIZATIONS_DIRECTORY)
        .map_err(SessionStoreError::InvalidSessionLayout)?;
    let scratch = resolve_direct_directory(&root, SCRATCH_DIRECTORY)
        .map_err(SessionStoreError::InvalidSessionLayout)?;
    let runs = resolve_direct_directory(&root, RUNS_DIRECTORY)
        .map_err(SessionStoreError::InvalidSessionLayout)?;
    Ok(SessionLayout {
        session_id,
        root,
        materializations,
        scratch,
        runs,
    })
}

fn remove_tombstone(parent: &Path, session_id: &SessionId) -> Result<(), SessionStoreError> {
    let tombstone = canonical_direct_directory(
        &parent.join(session_id.as_str()),
        parent,
        session_id.as_str(),
    )
    .map_err(SessionStoreError::InvalidTombstone)?;
    remove_exact_entry(&tombstone).map_err(SessionStoreError::DeleteTombstone)
}

fn path_exists(path: &Path) -> Result<bool, SessionStoreError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(SessionStoreError::InspectDeletionState(error)),
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

    // std::fs::rename may replace an empty destination directory on Unix.
    renameat_with(CWD, source, CWD, destination, RenameFlags::NOREPLACE).map_err(io::Error::from)
}

#[cfg(windows)]
fn rename_no_replace(source: &Path, destination: &Path) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

    // Windows canonicalize 生成支持长路径的 verbatim path；只解析父目录，避免跟随末级 junction。
    let source = canonical_parent_with_lexical_name(
        source,
        "deletion source has no final path component",
        "deletion source has no parent",
    )?;
    let destination = canonical_parent_with_lexical_name(
        destination,
        "deletion tombstone has no final path component",
        "deletion tombstone has no parent",
    )?;
    let source = null_terminated_wide_path(&source)?;
    let destination = null_terminated_wide_path(&destination)?;
    // `std::fs::rename` has no portable no-replace contract for directories.
    // SAFETY: both buffers are NUL-terminated and remain alive for the call.
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn canonical_parent_with_lexical_name(
    path: &Path,
    missing_name: &'static str,
    missing_parent: &'static str,
) -> io::Result<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, missing_name))?;
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, missing_parent))?;
    Ok(fs::canonicalize(parent)?.join(name))
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
        "Analysis Session deletion is supported only on Linux and Windows",
    ))
}

fn ensure_direct_directory(parent: &Path, name: &str) -> Result<PathBuf, DirectPathError> {
    match fs::create_dir(parent.join(name)) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(DirectPathError::Io(error)),
    }
    resolve_direct_directory(parent, name)
}

fn create_direct_directory(parent: &Path, name: &str) -> Result<PathBuf, DirectPathError> {
    let path = parent.join(name);
    fs::create_dir(&path).map_err(DirectPathError::Io)?;
    canonical_direct_directory(&path, parent, name)
}

fn resolve_direct_directory(parent: &Path, name: &str) -> Result<PathBuf, DirectPathError> {
    canonical_direct_directory(&parent.join(name), parent, name)
}

fn canonical_direct_directory(
    path: &Path,
    parent: &Path,
    name: &str,
) -> Result<PathBuf, DirectPathError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            DirectPathError::Missing(error)
        } else {
            DirectPathError::Io(error)
        }
    })?;
    if !metadata.file_type().is_dir()
        || metadata.file_type().is_symlink()
        || metadata_is_reparse_point(&metadata)
    {
        return Err(DirectPathError::Invalid);
    }
    let canonical = dunce::canonicalize(path).map_err(DirectPathError::Io)?;
    if canonical.parent() != Some(parent)
        || canonical.file_name().and_then(|value| value.to_str()) != Some(name)
    {
        return Err(DirectPathError::Invalid);
    }
    Ok(canonical)
}

fn validate_direct_file(
    path: &Path,
    parent: &Path,
    name: &str,
) -> Result<PathBuf, DirectPathError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            DirectPathError::Missing(error)
        } else {
            DirectPathError::Io(error)
        }
    })?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata_is_reparse_point(&metadata)
    {
        return Err(DirectPathError::Invalid);
    }
    let canonical = dunce::canonicalize(path).map_err(DirectPathError::Io)?;
    if canonical.parent() != Some(parent)
        || canonical.file_name().and_then(|value| value.to_str()) != Some(name)
    {
        return Err(DirectPathError::Invalid);
    }
    Ok(canonical)
}

fn read_direct_file(path: &Path, parent: &Path, name: &str) -> Result<File, DirectPathError> {
    File::open(validate_direct_file(path, parent, name)?).map_err(DirectPathError::Io)
}

fn open_lease_file(path: &Path, parent: &Path, name: &str) -> Result<File, DirectPathError> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(validate_direct_file(path, parent, name)?)
        .map_err(DirectPathError::Io)
}

#[cfg(windows)]
pub(super) fn metadata_is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
pub(super) fn metadata_is_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(windows)]
fn metadata_is_directory_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    metadata.file_attributes() & (0x400 | 0x10) == (0x400 | 0x10)
}

#[cfg(not(windows))]
fn metadata_is_directory_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}

#[derive(Debug, Error)]
pub(super) enum DirectPathError {
    #[error("required path does not exist")]
    Missing(#[source] io::Error),
    #[error("path is not a direct ordinary filesystem entry")]
    Invalid,
    #[error("filesystem access failed")]
    Io(#[source] io::Error),
}

fn diagnostic_safe_argument(value: &str) -> String {
    let mut rendered = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_control() {
            rendered.extend(character.escape_default());
        } else {
            rendered.push(character);
        }
    }
    rendered
}

#[derive(Debug, Error, Diagnostic)]
pub(super) enum SessionStoreError {
    #[error("Analysis Session {session_id} does not exist")]
    #[diagnostic(help("Use the exact Session ID returned by a successful `kat run`"))]
    NotFound { session_id: String },
    #[error("Analysis Session {session_id} is being deleted")]
    #[diagnostic(help("Wait for deletion to finish or start a new Analysis Session"))]
    Unavailable { session_id: String },
    #[error("Analysis Session {session_id} is in use")]
    #[diagnostic(help("Wait for active Run, Query, or inspection operations to finish and retry"))]
    InUse { session_id: String },
    #[error("Analysis Session is corrupted")]
    #[diagnostic(help("Do not use or delete the Session until its filesystem layout is repaired"))]
    Corrupted,
    #[error("Analysis Session storage is unavailable")]
    ResolveDataHome(#[source] io::Error),
    #[error("Analysis Session storage layout is invalid")]
    PrepareStorage(#[source] DirectPathError),
    #[error("failed to create the Analysis Session lease")]
    CreateLease {
        #[source]
        source: io::Error,
    },
    #[error("failed to lock the new Analysis Session lease")]
    CreateLeaseLock,
    #[error("Analysis Session lease is invalid")]
    ResolveLease(#[source] DirectPathError),
    #[error("failed to lock the Analysis Session lease")]
    LockLease(#[source] io::Error),
    #[error("failed to create the Analysis Session")]
    CreateSession {
        #[source]
        source: io::Error,
    },
    #[error("Analysis Session layout is invalid")]
    PrepareSessionLayout(#[source] DirectPathError),
    #[error("Analysis Session layout is invalid")]
    InvalidSessionLayout(#[source] DirectPathError),
    #[error("Analysis Session marker is invalid")]
    DecodeSessionMarker(#[source] serde_json::Error),
    #[error("Analysis Session marker identity does not match its address")]
    InvalidSessionIdentity,
    #[error("failed to create a private Run candidate")]
    CreateRunCandidate(#[source] io::Error),
    #[error("private Run candidate layout is invalid")]
    PrepareRunCandidate(#[source] DirectPathError),
    #[error("failed to remove the private Run scratch directory")]
    #[diagnostic(help("Provide writable storage and retry without publishing a partial Run"))]
    CleanScratch(#[source] DirectPathError),
    #[error("Workflow Runtime wrote the CLI-owned Analysis Session marker")]
    #[diagnostic(help("Inspect the Operation log and repair the bundled Runtime deployment"))]
    PrematureSessionMarker,
    #[error("failed to create a temporary Analysis Session marker")]
    CreateSessionMarker(#[source] io::Error),
    #[error("failed to encode the Analysis Session marker")]
    EncodeSessionMarker(#[source] serde_json::Error),
    #[error("failed to write the Analysis Session marker")]
    WriteSessionMarker(#[source] io::Error),
    #[error("failed to durably flush the Analysis Session marker")]
    FlushSessionMarker(#[source] io::Error),
    #[error("failed to publish the Analysis Session marker")]
    #[diagnostic(help("Provide writable storage and retry the complete Run"))]
    PublishSessionMarker(#[source] io::Error),
    #[error("failed to inspect Analysis Session deletion state")]
    InspectDeletionState(#[source] io::Error),
    #[error("Analysis Session deletion state is corrupted")]
    #[diagnostic(help("Ensure only the Session or its fixed tombstone exists, then retry"))]
    InvalidDeletionState,
    #[error("failed to move Analysis Session into its deletion tombstone")]
    MoveToTombstone(#[source] io::Error),
    #[error("Analysis Session deletion tombstone is invalid")]
    InvalidTombstone(#[source] DirectPathError),
    #[error("failed to permanently delete Analysis Session tombstone")]
    #[diagnostic(help("Retry `kat session delete` with the same Session ID"))]
    DeleteTombstone(#[source] io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION_ID: &str = "019f6e00-0000-7000-8000-000000000060";
    const RUN_ID: &str = "019f6e00-0000-7000-8000-000000000061";

    #[test]
    fn new_session_collision_never_removes_a_root_it_did_not_create() {
        let temporary = tempfile::tempdir().unwrap();
        let sessions = temporary.path().join(SESSIONS_DIRECTORY);
        let existing = sessions.join(SESSION_ID);
        fs::create_dir_all(sessions.join(LEASES_DIRECTORY)).unwrap();
        fs::create_dir_all(sessions.join(DELETIONS_DIRECTORY)).unwrap();
        fs::create_dir(&existing).unwrap();
        fs::write(
            existing.join("sentinel"),
            b"belongs to an earlier candidate",
        )
        .unwrap();

        let error = SessionStore::new(temporary.path())
            .create_run(
                SessionId::parse(SESSION_ID).unwrap(),
                RunId::parse(RUN_ID).unwrap(),
            )
            .err()
            .expect("an existing root must win the no-replace creation race");

        assert!(matches!(error, SessionStoreError::CreateSession { .. }));
        assert_eq!(
            fs::read(existing.join("sentinel")).unwrap(),
            b"belongs to an earlier candidate"
        );
        assert!(
            !sessions
                .join(LEASES_DIRECTORY)
                .join(format!("{SESSION_ID}.lock"))
                .exists()
        );
    }

    #[test]
    fn created_scratch_is_removed_when_direct_entry_validation_fails() {
        let temporary = tempfile::tempdir().unwrap();
        let sessions = temporary.path().join(SESSIONS_DIRECTORY);
        let root = sessions.join(SESSION_ID);
        let materializations = root.join(MATERIALIZATIONS_DIRECTORY);
        let scratch_root = root.join(SCRATCH_DIRECTORY);
        let runs = root.join(RUNS_DIRECTORY);
        fs::create_dir_all(&materializations).unwrap();
        fs::create_dir(&scratch_root).unwrap();
        fs::create_dir(&runs).unwrap();
        let layout = SessionLayout {
            session_id: SessionId::parse(SESSION_ID).unwrap(),
            root: dunce::canonicalize(root).unwrap(),
            materializations: dunce::canonicalize(materializations).unwrap(),
            scratch: dunce::canonicalize(scratch_root).unwrap(),
            runs: dunce::canonicalize(runs).unwrap(),
        };
        let scratch = layout.scratch.join(RUN_ID);
        fs::create_dir(&scratch).unwrap();
        fs::remove_dir(&scratch).unwrap();
        fs::write(&scratch, b"Runtime-owned replacement").unwrap();

        let error = validate_created_scratch(&layout, &scratch, RUN_ID).unwrap_err();

        assert!(matches!(error, DirectPathError::Invalid));
        assert!(!scratch.exists());
    }

    #[cfg(windows)]
    #[test]
    fn deletion_path_uses_a_verbatim_parent_and_lexical_final_name() {
        use std::path::{Component, Prefix};

        let temporary = tempfile::tempdir().unwrap();
        let parent = temporary.path().join("sessions");
        fs::create_dir(&parent).unwrap();
        let missing_final_entry = parent.join(SESSION_ID);

        let prepared = canonical_parent_with_lexical_name(
            &missing_final_entry,
            "deletion source has no final path component",
            "deletion source has no parent",
        )
        .unwrap();

        assert_eq!(prepared.file_name(), missing_final_entry.file_name());
        assert!(!prepared.exists());
        assert!(matches!(
            prepared.components().next(),
            Some(Component::Prefix(prefix))
                if matches!(prefix.kind(), Prefix::VerbatimDisk(_) | Prefix::VerbatimUNC(..))
        ));
        assert_eq!(
            null_terminated_wide_path(&prepared).unwrap().last(),
            Some(&0)
        );
    }
}
