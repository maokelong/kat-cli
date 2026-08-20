use std::{
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use arrow_array::{RecordBatch, StringArray, UInt64Array};
use arrow_schema::{DataType, Field, Schema};

use crate::{
    DatasetWriteTarget,
    dataset_writer::DatasetWriter,
    formats::ftrace_text::{TextFtraceClock, UnsupportedFtraceEvent, decode_reader},
    proto::TracePluginConfig,
    protobuf_source::SpoolOptions,
    text_ftrace_source_capture::TextFtraceSourceCapture,
};

const TICKS_PER_SECOND: u64 = 1_000_000_000;

#[derive(Debug)]
pub struct ImportedTextFtrace {
    path: PathBuf,
    unsupported_events: Vec<UnsupportedFtraceEvent>,
}

impl ImportedTextFtrace {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn unsupported_events(&self) -> &[UnsupportedFtraceEvent] {
        &self.unsupported_events
    }
}

#[derive(Debug, thiserror::Error)]
#[error("text ftrace Import failed: {source:#}")]
pub struct TextFtraceImportError {
    #[source]
    source: anyhow::Error,
}

impl TextFtraceImportError {
    pub fn compatibility(&self) -> Option<&crate::TextFtraceCompatibilityError> {
        self.source
            .chain()
            .find_map(|error| error.downcast_ref::<crate::TextFtraceCompatibilityError>())
    }
}

pub fn import_text_ftrace(
    path: impl AsRef<Path>,
    clock: TextFtraceClock,
    target: DatasetWriteTarget,
) -> std::result::Result<ImportedTextFtrace, TextFtraceImportError> {
    import_text_ftrace_inner(path.as_ref(), clock, target)
        .map_err(|source| TextFtraceImportError { source })
}

fn import_text_ftrace_inner(
    path: &Path,
    clock: TextFtraceClock,
    target: DatasetWriteTarget,
) -> Result<ImportedTextFtrace> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let config = TracePluginConfig {
        clock: clock.proto_clock().to_owned(),
        ..Default::default()
    };
    let mut capture = TextFtraceSourceCapture::new(SpoolOptions::default(), &config)?;
    let summary = decode_reader(BufReader::new(file), |cpu, event| {
        capture.append_event(cpu, &event)
    })
    .with_context(|| format!("failed to parse {}", path.display()))?;
    let prepared = capture
        .finish()
        .with_context(|| format!("failed to prepare {}", path.display()))?;
    let unsupported_events = summary.unsupported_events();
    let mut writer = DatasetWriter::begin(target)?;
    write_clock_domain(&mut writer, clock)?;
    prepared.write_into(&mut writer)?;
    let path = writer.finish()?;
    Ok(ImportedTextFtrace {
        path,
        unsupported_events,
    })
}

fn write_clock_domain(writer: &mut DatasetWriter, clock: TextFtraceClock) -> Result<()> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("clock_domain", DataType::Utf8, false),
        Field::new("clock_type", DataType::Utf8, false),
        Field::new("ticks_per_second", DataType::UInt64, false),
    ]));
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(StringArray::from(vec![clock.domain()])),
            Arc::new(StringArray::from(vec![clock.domain()])),
            Arc::new(UInt64Array::from(vec![TICKS_PER_SECOND])),
        ],
    )?;
    let mut table = writer.begin_table("clock_domain", schema)?;
    table.write(&batch)?;
    table.finish()?;
    Ok(())
}
