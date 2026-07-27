use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use thiserror::Error;

#[derive(Debug)]
pub(crate) struct OperationLog {
    path: PathBuf,
    file: File,
}

impl OperationLog {
    pub(crate) fn create(
        data_home: &Path,
        file_prefix: &str,
        write_header: impl FnOnce(&mut dyn Write) -> io::Result<()>,
    ) -> Result<Self, OperationLogError> {
        Self::create_with(data_home, file_prefix, |file| write_header(file))
    }

    pub(crate) fn create_run(
        data_home: &Path,
        candidate_id: &str,
        write_header: impl FnOnce(&mut dyn Write) -> io::Result<()>,
    ) -> Result<Self, OperationLogError> {
        let directory = data_home.join("logs");
        fs::create_dir_all(&directory).map_err(|source| OperationLogError::CreateDirectory {
            path: directory.clone(),
            source,
        })?;
        let path = directory.join(format!("run-{candidate_id}.log"));
        Self::create_at(path, |file| write_header(file))
    }

    fn create_with(
        data_home: &Path,
        file_prefix: &str,
        write_header: impl FnOnce(&mut File) -> io::Result<()>,
    ) -> Result<Self, OperationLogError> {
        let directory = data_home.join("logs");
        fs::create_dir_all(&directory).map_err(|source| OperationLogError::CreateDirectory {
            path: directory.clone(),
            source,
        })?;
        let path = directory.join(format!("{file_prefix}-{}.log", uuid::Uuid::now_v7()));
        Self::create_at(path, write_header)
    }

    fn create_at(
        path: PathBuf,
        write_header: impl FnOnce(&mut File) -> io::Result<()>,
    ) -> Result<Self, OperationLogError> {
        let directory = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|source| OperationLogError::Create { directory, source })?;
        write_header(&mut file).map_err(|source| OperationLogError::Write {
            path: path.clone(),
            source,
        })?;
        Ok(Self { path, file })
    }

    pub(crate) fn finish(self) -> Result<String, OperationLogError> {
        self.finish_with(File::flush)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn append(&mut self, bytes: &[u8]) -> Result<(), OperationLogError> {
        self.file
            .write_all(bytes)
            .map_err(|source| OperationLogError::Write {
                path: self.path.clone(),
                source,
            })
    }

    fn finish_with(
        mut self,
        flush: impl FnOnce(&mut File) -> io::Result<()>,
    ) -> Result<String, OperationLogError> {
        flush(&mut self.file).map_err(|source| OperationLogError::Flush {
            path: self.path.clone(),
            source,
        })?;
        drop(self.file);
        let path =
            dunce::canonicalize(&self.path).map_err(|source| OperationLogError::Canonicalize {
                path: self.path.clone(),
                source,
            })?;
        path
        .to_str()
        .map(str::to_owned)
        .ok_or(OperationLogError::NonUnicode { path })
    }
}

impl Write for OperationLog {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.file.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

#[derive(Debug, Error)]
pub(crate) enum OperationLogError {
    #[error("failed to create Operation log directory {path:?}")]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to create Operation log in {directory:?}")]
    Create {
        directory: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to write Operation log")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to flush Operation log")]
    Flush {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to resolve Operation log")]
    Canonicalize {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Operation log path cannot be represented as native Unicode")]
    NonUnicode { path: PathBuf },
}

impl OperationLogError {
    pub(crate) fn readable_path(&self) -> Option<String> {
        let path = match self {
            Self::Write { path, .. }
            | Self::Flush { path, .. }
            | Self::Canonicalize { path, .. }
            | Self::NonUnicode { path } => path,
            Self::CreateDirectory { .. } | Self::Create { .. } => return None,
        };
        if !fs::metadata(path).ok()?.is_file() || File::open(path).is_err() {
            return None;
        }
        dunce::canonicalize(path).ok()?.to_str().map(str::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_creation_failure_identifies_the_safe_storage_target() {
        let temp = tempfile::tempdir().unwrap();
        let logs = temp.path().join("logs");
        fs::write(&logs, "blocks directory creation").unwrap();

        let error = OperationLog::create(temp.path(), "test", |_| Ok(())).unwrap_err();

        assert!(matches!(error, OperationLogError::CreateDirectory { .. }));
        assert!(error.to_string().contains(&format!("{logs:?}")));
        assert!(error.readable_path().is_none());
    }

    #[test]
    fn file_creation_failure_identifies_only_the_safe_parent_directory() {
        let temp = tempfile::tempdir().unwrap();
        let candidate_id = "019f6e00-0000-7000-8000-000000000004";
        let path = temp.path().join(format!("run-{candidate_id}.log"));
        fs::write(&path, "existing").unwrap();

        let error = OperationLog::create_at(path, |_| Ok(())).unwrap_err();
        let diagnostic = error.to_string();

        assert!(matches!(error, OperationLogError::Create { .. }));
        assert!(diagnostic.contains(&format!("{:?}", temp.path())));
        assert!(!diagnostic.contains(candidate_id));
        assert!(error.readable_path().is_none());
        assert_eq!(
            std::error::Error::source(&error)
                .and_then(|source| source.downcast_ref::<io::Error>())
                .map(io::Error::kind),
            Some(io::ErrorKind::AlreadyExists)
        );
    }

    #[test]
    fn fault_seams_distinguish_partial_files_from_create_failures() {
        let temp = tempfile::tempdir().unwrap();
        let write_error = OperationLog::create_with(temp.path(), "test", |_| {
            Err(io::Error::other("injected header write failure"))
        })
        .unwrap_err();
        assert!(matches!(write_error, OperationLogError::Write { .. }));
        assert!(write_error.readable_path().is_some());

        let log =
            OperationLog::create_with(temp.path(), "test", |file| file.write_all(b"partial\n"))
                .unwrap();
        let flush_error = log
            .finish_with(|_| Err(io::Error::other("injected flush failure")))
            .unwrap_err();
        assert!(matches!(flush_error, OperationLogError::Flush { .. }));
        assert!(flush_error.readable_path().is_some());

        let unrelated = temp.path().join("unrelated.log");
        fs::write(&unrelated, "not this operation").unwrap();
        let create_error = OperationLogError::Create {
            directory: temp.path().to_path_buf(),
            source: io::Error::new(io::ErrorKind::AlreadyExists, "injected create failure"),
        };
        assert!(create_error.readable_path().is_none());
    }
}
