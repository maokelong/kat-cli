use std::{cell::RefCell, collections::HashSet, fs::File, path::PathBuf, rc::Rc, sync::Arc};

use anyhow::{Context, Result, bail};
use arrow_array::RecordBatch;
use arrow_schema::Schema;
use parquet::arrow::{
    ArrowWriter,
    arrow_reader::{ArrowReaderMetadata, ArrowReaderOptions},
};

use crate::relation_name::valid_relation_name;

const MATERIALIZATION_VERSION_METADATA_KEY: &str = "kat.materialization.version";

/// Owns the complete set of flat Parquet relations for one private staging root.
#[derive(Clone)]
pub(crate) struct RelationWriter {
    root: PathBuf,
    materialization_version: &'static str,
    names: Rc<RefCell<HashSet<String>>>,
}

impl RelationWriter {
    pub(crate) fn new(root: impl Into<PathBuf>, materialization_version: &'static str) -> Self {
        Self {
            root: root.into(),
            materialization_version,
            names: Rc::new(RefCell::new(HashSet::new())),
        }
    }

    pub(crate) fn begin(&self, name: &str, schema: Arc<Schema>) -> Result<RelationFileWriter> {
        if !valid_relation_name(name) {
            bail!("invalid relation name {name:?}");
        }
        if !self.names.borrow_mut().insert(name.to_owned()) {
            bail!("duplicate relation name {name:?}");
        }

        let mut columns = HashSet::new();
        for field in schema.fields() {
            if !columns.insert(field.name().as_str()) {
                bail!(
                    "relation {name:?} has duplicate top-level column {:?}",
                    field.name()
                );
            }
        }

        let mut metadata = schema.metadata().clone();
        metadata.insert(
            MATERIALIZATION_VERSION_METADATA_KEY.to_owned(),
            self.materialization_version.to_owned(),
        );
        let schema = Arc::new(schema.as_ref().clone().with_metadata(metadata));

        let path = self.root.join(format!("{name}.parquet"));
        let file = File::create(&path)
            .with_context(|| format!("failed to create relation {name:?} at {}", path.display()))?;
        let writer = ArrowWriter::try_new(file, schema, None).with_context(|| {
            format!(
                "failed to open Parquet writer for relation {name:?} at {}",
                path.display()
            )
        })?;
        Ok(RelationFileWriter {
            name: name.to_owned(),
            path,
            writer,
        })
    }

    pub(crate) fn validate(&self) -> Result<()> {
        let mut names = self.names.borrow().iter().cloned().collect::<Vec<_>>();
        names.sort();
        for name in names {
            let path = self.root.join(format!("{name}.parquet"));
            let file = File::open(&path).with_context(|| {
                format!(
                    "failed to reopen staged relation {name:?} at {}",
                    path.display()
                )
            })?;
            ArrowReaderMetadata::load(&file, ArrowReaderOptions::default()).with_context(|| {
                format!(
                    "failed to validate staged relation {name:?} at {}",
                    path.display()
                )
            })?;
        }
        Ok(())
    }
}

pub(crate) struct RelationFileWriter {
    name: String,
    path: PathBuf,
    writer: ArrowWriter<File>,
}

impl RelationFileWriter {
    pub(crate) fn write(&mut self, batch: &RecordBatch) -> Result<()> {
        self.writer.write(batch).with_context(|| {
            format!(
                "failed to write relation {:?} at {}",
                self.name,
                self.path.display()
            )
        })
    }

    pub(crate) fn finish(self) -> Result<()> {
        self.writer.close().with_context(|| {
            format!(
                "failed to close relation {:?} at {}",
                self.name,
                self.path.display()
            )
        })?;
        Ok(())
    }
}
