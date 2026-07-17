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

    pub(crate) fn create_test(
        data_home: &Path,
        token: &str,
        write_header: impl FnOnce(&mut dyn Write) -> io::Result<()>,
    ) -> Result<Self, OperationLogError> {
        let directory = data_home.join("logs");
        fs::create_dir_all(&directory).map_err(|source| OperationLogError::CreateDirectory {
            path: directory.clone(),
            source,
        })?;
        Self::create_at(directory.join(format!("test-{token}.log")), |file| {
            write_header(file)
        })
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
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|source| OperationLogError::Create {
                path: path.clone(),
                source,
            })?;
        write_header(&mut file).map_err(|source| OperationLogError::Write {
            path: path.clone(),
            source,
        })?;
        Ok(Self { path, file })
    }

    pub(crate) fn finish(self) -> Result<String, OperationLogError> {
        self.finish_with(File::flush)
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
        path.to_str()
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
    #[error("failed to create Operation log directory {path}")]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to create Operation log {path}")]
    Create {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to write Operation log {path}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to flush Operation log {path}")]
    Flush {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to resolve Operation log {path}")]
    Canonicalize {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Operation log path cannot be represented as native Unicode: {path:?}")]
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
            path: unrelated,
            source: io::Error::new(io::ErrorKind::AlreadyExists, "injected create failure"),
        };
        assert!(create_error.readable_path().is_none());
    }
}
