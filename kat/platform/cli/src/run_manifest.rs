use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io,
    path::{Path, PathBuf},
};

use arrow_schema::{DataType, Field, IntervalUnit, TimeUnit, UnionMode};
use miette::Diagnostic;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    session_store::{RunId, SessionLayout, metadata_is_reparse_point},
    workflow_runtime::{self, RunOutputMetadata},
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RunManifest {
    pub(super) session_id: String,
    pub(super) run_id: String,
    pub(super) pack: String,
    pub(super) workflow: String,
    pub(super) child_runs: Vec<String>,
    #[serde(
        default,
        rename = "dataset",
        deserialize_with = "deserialize_ignored",
        skip_serializing
    )]
    _legacy_dataset: (),
    pub(super) inputs: BTreeMap<String, serde_json::Value>,
    pub(super) outputs: BTreeMap<String, RunOutputMetadata>,
}

impl RunManifest {
    pub(super) fn new(
        session_id: String,
        run_id: String,
        pack: String,
        workflow: String,
        mut child_runs: Vec<String>,
        inputs: BTreeMap<String, serde_json::Value>,
        outputs: BTreeMap<String, RunOutputMetadata>,
    ) -> Self {
        child_runs.sort();
        Self {
            session_id,
            run_id,
            pack,
            workflow,
            child_runs,
            _legacy_dataset: (),
            inputs,
            outputs,
        }
    }
}

fn deserialize_ignored<'de, D>(deserializer: D) -> Result<(), D::Error>
where
    D: serde::Deserializer<'de>,
{
    serde::de::IgnoredAny::deserialize(deserializer).map(drop)
}

pub(super) struct PublishedRun {
    pub(super) run_id: String,
    pub(super) pack: String,
    pub(super) workflow: String,
    pub(super) child_runs: Vec<String>,
    pub(super) outputs: BTreeMap<String, RunOutputMetadata>,
    pub(super) output_paths: BTreeMap<String, String>,
}

pub(super) fn resolve(
    session: &SessionLayout,
    run: &str,
) -> Result<PublishedRun, PublishedRunError> {
    let run_id = RunId::parse(run).ok_or_else(|| PublishedRunError::NotFound {
        session_id: session.session_id().as_str().to_owned(),
        run_id: diagnostic_safe_argument(run),
    })?;
    let run_path = session.runs().join(run_id.as_str());
    match fs::symlink_metadata(&run_path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(PublishedRunError::NotFound {
                session_id: session.session_id().as_str().to_owned(),
                run_id: run_id.as_str().to_owned(),
            });
        }
        Err(error) => return Err(PublishedRunError::CorruptPath(error)),
        Ok(metadata)
            if !metadata.file_type().is_dir()
                || metadata.file_type().is_symlink()
                || metadata_is_reparse_point(&metadata) =>
        {
            return Err(PublishedRunError::InvalidLayout);
        }
        Ok(_) => {}
    }
    let run_path = canonical_direct_directory(&run_path, session.runs(), run_id.as_str())?;
    let manifest_path = run_path.join("manifest.json");
    match fs::symlink_metadata(&manifest_path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(PublishedRunError::NotFound {
                session_id: session.session_id().as_str().to_owned(),
                run_id: run_id.as_str().to_owned(),
            });
        }
        Err(error) => return Err(PublishedRunError::CorruptPath(error)),
        Ok(metadata)
            if !metadata.file_type().is_file()
                || metadata.file_type().is_symlink()
                || metadata_is_reparse_point(&metadata) =>
        {
            return Err(PublishedRunError::InvalidLayout);
        }
        Ok(_) => {}
    }
    let manifest_path = canonical_direct_file(&manifest_path, &run_path, "manifest.json")?;
    let bytes = fs::read(manifest_path).map_err(PublishedRunError::ReadManifest)?;
    let manifest: RunManifest =
        serde_json::from_slice(&bytes).map_err(PublishedRunError::DecodeManifest)?;
    validate(&manifest, session.session_id().as_str(), run_id.as_str())?;
    let output_paths = resolve_outputs(&run_path, &manifest)?;
    Ok(PublishedRun {
        run_id: manifest.run_id,
        pack: manifest.pack,
        workflow: manifest.workflow,
        child_runs: manifest.child_runs,
        outputs: manifest.outputs,
        output_paths,
    })
}

pub(super) fn resolve_all(session: &SessionLayout) -> Result<Vec<PublishedRun>, PublishedRunError> {
    let entries = fs::read_dir(session.runs()).map_err(PublishedRunError::ReadRuns)?;
    let mut selected = Vec::new();
    for entry in entries {
        let entry = entry.map_err(PublishedRunError::ReadRuns)?;
        let path = entry.path();
        let Some(metadata) = optional_unselected_candidate(fs::symlink_metadata(&path))? else {
            continue;
        };
        if metadata.file_type().is_symlink() || metadata_is_reparse_point(&metadata) {
            return Err(PublishedRunError::InvalidLayout);
        }
        if !metadata.file_type().is_dir() {
            continue;
        }
        let Some(canonical) = optional_unselected_candidate(dunce::canonicalize(&path))? else {
            continue;
        };
        if canonical.parent() != Some(session.runs()) {
            return Err(PublishedRunError::InvalidLayout);
        }
        match fs::symlink_metadata(canonical.join("manifest.json")) {
            Ok(_) => {
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| PublishedRunError::InvalidLayout)?;
                selected.push(name);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(PublishedRunError::CorruptPath(error)),
        }
    }
    selected.sort();
    let runs = selected
        .into_iter()
        .map(|run_id| resolve(session, &run_id))
        .collect::<Result<Vec<_>, _>>()?;
    complete_child_run_references(session, runs)
}

fn complete_child_run_references(
    session: &SessionLayout,
    runs: Vec<PublishedRun>,
) -> Result<Vec<PublishedRun>, PublishedRunError> {
    let mut pending = runs
        .iter()
        .flat_map(|run| run.child_runs.iter().cloned())
        .collect::<BTreeSet<_>>();
    let mut published = runs
        .into_iter()
        .map(|run| (run.run_id.clone(), run))
        .collect::<BTreeMap<_, _>>();
    while let Some(child_run_id) = pending.pop_first() {
        if published.contains_key(&child_run_id) {
            continue;
        }
        let child = match resolve(session, &child_run_id) {
            Ok(child) => child,
            Err(PublishedRunError::NotFound { .. }) => {
                return Err(PublishedRunError::InvalidFacts);
            }
            Err(error) => return Err(error),
        };
        pending.extend(child.child_runs.iter().cloned());
        published.insert(child_run_id, child);
    }
    Ok(published.into_values().collect())
}

/// Verifies the complete Runtime-owned Output directory before the Host publishes a Manifest.
pub(super) fn validate_candidate_outputs(
    candidate: &Path,
    outputs: &BTreeMap<String, RunOutputMetadata>,
) -> Result<(), PublishedRunError> {
    resolve_output_paths(candidate, outputs).map(drop)
}

fn optional_unselected_candidate<T>(result: io::Result<T>) -> Result<Option<T>, PublishedRunError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(PublishedRunError::CorruptPath(error)),
    }
}

fn resolve_outputs(
    run_path: &Path,
    manifest: &RunManifest,
) -> Result<BTreeMap<String, String>, PublishedRunError> {
    resolve_output_paths(run_path, &manifest.outputs)
}

fn resolve_output_paths(
    run_path: &Path,
    outputs: &BTreeMap<String, RunOutputMetadata>,
) -> Result<BTreeMap<String, String>, PublishedRunError> {
    let output_directory =
        canonical_direct_directory(&run_path.join("outputs"), run_path, "outputs")?;
    let expected = outputs
        .keys()
        .map(|name| format!("{name}.parquet").into())
        .collect::<BTreeSet<_>>();
    let observed = fs::read_dir(&output_directory)
        .map_err(PublishedRunError::CorruptPath)?
        .map(|entry| {
            entry
                .map(|entry| entry.file_name())
                .map_err(PublishedRunError::CorruptPath)
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if observed != expected {
        return Err(PublishedRunError::InvalidLayout);
    }
    outputs
        .iter()
        .map(|(name, metadata)| {
            let file_name = format!("{name}.parquet");
            let output = canonical_direct_file(
                &output_directory.join(&file_name),
                &output_directory,
                &file_name,
            )?;
            validate_output_footer(&output, metadata)?;
            let path = output
                .to_str()
                .map(str::to_owned)
                .ok_or(PublishedRunError::NonUnicodePath)?;
            Ok((name.clone(), path))
        })
        .collect()
}

fn validate_output_footer(
    path: &Path,
    expected: &RunOutputMetadata,
) -> Result<(), PublishedRunError> {
    let file = File::open(path).map_err(PublishedRunError::CorruptPath)?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)
        .map_err(PublishedRunError::InvalidParquet)?;
    let row_count = u64::try_from(reader.metadata().file_metadata().num_rows())
        .map_err(|_| PublishedRunError::InvalidFacts)?;
    let fields = reader.schema().fields();
    if row_count != expected.row_count || fields.len() != expected.columns.len() {
        return Err(PublishedRunError::InvalidFacts);
    }
    for (field, column) in fields.iter().zip(&expected.columns) {
        if field.name() != &column.name {
            return Err(PublishedRunError::InvalidFacts);
        }
        if !public_arrow_type_matches(field.data_type(), &column.data_type) {
            return Err(PublishedRunError::InvalidFacts);
        }
    }
    Ok(())
}

fn public_arrow_type_matches(data_type: &DataType, declared: &str) -> bool {
    public_arrow_type(data_type)
        .is_some_and(|actual| actual == declared.replace(", ordered=1>", ", ordered=0>"))
}

fn public_arrow_type(data_type: &DataType) -> Option<String> {
    let rendered = match data_type {
        DataType::Null => "null".to_owned(),
        DataType::Boolean => "bool".to_owned(),
        DataType::Int8 => "int8".to_owned(),
        DataType::Int16 => "int16".to_owned(),
        DataType::Int32 => "int32".to_owned(),
        DataType::Int64 => "int64".to_owned(),
        DataType::UInt8 => "uint8".to_owned(),
        DataType::UInt16 => "uint16".to_owned(),
        DataType::UInt32 => "uint32".to_owned(),
        DataType::UInt64 => "uint64".to_owned(),
        DataType::Float16 => "halffloat".to_owned(),
        DataType::Float32 => "float".to_owned(),
        DataType::Float64 => "double".to_owned(),
        DataType::Timestamp(unit, timezone) => match timezone {
            Some(timezone) => format!("timestamp[{}, tz={timezone}]", public_time_unit(*unit)),
            None => format!("timestamp[{}]", public_time_unit(*unit)),
        },
        DataType::Date32 => "date32[day]".to_owned(),
        DataType::Date64 => "date64[ms]".to_owned(),
        DataType::Time32(unit) => format!("time32[{}]", public_time_unit(*unit)),
        DataType::Time64(unit) => format!("time64[{}]", public_time_unit(*unit)),
        DataType::Duration(unit) => format!("duration[{}]", public_time_unit(*unit)),
        DataType::Interval(IntervalUnit::YearMonth) => "month_interval".to_owned(),
        DataType::Interval(IntervalUnit::DayTime) => "day_time_interval".to_owned(),
        DataType::Interval(IntervalUnit::MonthDayNano) => "month_day_nano_interval".to_owned(),
        DataType::Binary => "binary".to_owned(),
        DataType::FixedSizeBinary(size) => format!("fixed_size_binary[{size}]"),
        DataType::LargeBinary => "large_binary".to_owned(),
        DataType::BinaryView => "binary_view".to_owned(),
        DataType::Utf8 => "string".to_owned(),
        DataType::LargeUtf8 => "large_string".to_owned(),
        DataType::Utf8View => "string_view".to_owned(),
        DataType::List(field) => format!("list<{}>", public_arrow_field(field)?),
        DataType::ListView(field) => format!("list_view<{}>", public_arrow_field(field)?),
        DataType::FixedSizeList(field, size) => {
            format!("fixed_size_list<{}>[{size}]", public_arrow_field(field)?)
        }
        DataType::LargeList(field) => format!("large_list<{}>", public_arrow_field(field)?),
        DataType::LargeListView(field) => {
            format!("large_list_view<{}>", public_arrow_field(field)?)
        }
        DataType::Struct(fields) => format!(
            "struct<{}>",
            fields
                .iter()
                .map(|field| public_arrow_field(field))
                .collect::<Option<Vec<_>>>()?
                .join(", ")
        ),
        DataType::Union(fields, mode) => {
            let mode = match mode {
                UnionMode::Sparse => "sparse_union",
                UnionMode::Dense => "dense_union",
            };
            let fields = fields
                .iter()
                .map(|(type_id, field)| {
                    public_arrow_field(field).map(|field| format!("{field}={type_id}"))
                })
                .collect::<Option<Vec<_>>>()?
                .join(", ");
            format!("{mode}<{fields}>")
        }
        // Arrow Rust 不保留 PyArrow 的 `ordered` dictionary 标志；统一渲染为
        // 0，匹配时只规范化 PyArrow 完整类型中的该标志。
        DataType::Dictionary(indices, values) => format!(
            "dictionary<values={}, indices={}, ordered=0>",
            public_arrow_type(values)?,
            public_arrow_type(indices)?
        ),
        DataType::Decimal32(precision, scale) => format!("decimal32({precision}, {scale})"),
        DataType::Decimal64(precision, scale) => format!("decimal64({precision}, {scale})"),
        DataType::Decimal128(precision, scale) => format!("decimal128({precision}, {scale})"),
        DataType::Decimal256(precision, scale) => format!("decimal256({precision}, {scale})"),
        DataType::Map(entries, keys_sorted) => public_map_type(entries, *keys_sorted)?,
        DataType::RunEndEncoded(run_ends, values) => format!(
            "run_end_encoded<{}, {}>",
            public_arrow_field(run_ends)?,
            public_arrow_field(values)?
        ),
    };
    Some(rendered)
}

fn public_arrow_field(field: &Field) -> Option<String> {
    let nullable = if field.is_nullable() { "" } else { " not null" };
    Some(format!(
        "{}: {}{nullable}",
        field.name(),
        public_arrow_type(field.data_type())?
    ))
}

fn public_map_type(entries: &Field, keys_sorted: bool) -> Option<String> {
    let DataType::Struct(fields) = entries.data_type() else {
        return None;
    };
    let [key, value] = fields.as_ref() else {
        return None;
    };
    let key_name = (key.name() != "key").then(|| format!(" ('{}')", key.name()));
    let value_name = (value.name() != "value").then(|| format!(" ('{}')", value.name()));
    let sorted = if keys_sorted { ", keys_sorted" } else { "" };
    Some(format!(
        "map<{}{}, {}{}{sorted}>",
        public_arrow_type(key.data_type())?,
        key_name.as_deref().unwrap_or_default(),
        public_arrow_type(value.data_type())?,
        value_name.as_deref().unwrap_or_default(),
    ))
}

fn public_time_unit(unit: TimeUnit) -> &'static str {
    match unit {
        TimeUnit::Second => "s",
        TimeUnit::Millisecond => "ms",
        TimeUnit::Microsecond => "us",
        TimeUnit::Nanosecond => "ns",
    }
}

fn canonical_direct_directory(
    path: &Path,
    parent: &Path,
    name: &str,
) -> Result<PathBuf, PublishedRunError> {
    let metadata = fs::symlink_metadata(path).map_err(PublishedRunError::CorruptPath)?;
    if !metadata.file_type().is_dir()
        || metadata.file_type().is_symlink()
        || metadata_is_reparse_point(&metadata)
    {
        return Err(PublishedRunError::InvalidLayout);
    }
    let canonical = dunce::canonicalize(path).map_err(PublishedRunError::CorruptPath)?;
    if canonical.parent() != Some(parent)
        || canonical.file_name().and_then(|value| value.to_str()) != Some(name)
    {
        return Err(PublishedRunError::InvalidLayout);
    }
    Ok(canonical)
}

fn canonical_direct_file(
    path: &Path,
    parent: &Path,
    name: &str,
) -> Result<PathBuf, PublishedRunError> {
    let metadata = fs::symlink_metadata(path).map_err(PublishedRunError::CorruptPath)?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata_is_reparse_point(&metadata)
    {
        return Err(PublishedRunError::InvalidLayout);
    }
    let canonical = dunce::canonicalize(path).map_err(PublishedRunError::CorruptPath)?;
    if canonical.parent() != Some(parent)
        || canonical.file_name().and_then(|value| value.to_str()) != Some(name)
    {
        return Err(PublishedRunError::InvalidLayout);
    }
    Ok(canonical)
}

fn validate(
    manifest: &RunManifest,
    session_id: &str,
    run_id: &str,
) -> Result<(), PublishedRunError> {
    if manifest.session_id != session_id
        || manifest.run_id != run_id
        || manifest.pack.trim().is_empty()
        || manifest.workflow.trim().is_empty()
        || manifest.outputs.is_empty()
        || manifest
            .child_runs
            .iter()
            .any(|child_run| child_run == run_id || RunId::parse(child_run).is_none())
        || !manifest.child_runs.windows(2).all(|pair| pair[0] < pair[1])
    {
        return Err(PublishedRunError::InvalidFacts);
    }
    if manifest.inputs.iter().any(|(name, value)| {
        name.is_empty()
            || !matches!(
                value,
                serde_json::Value::Null
                    | serde_json::Value::Bool(_)
                    | serde_json::Value::Number(_)
                    | serde_json::Value::String(_)
            )
            || value.as_f64().is_some_and(|number| !number.is_finite())
    }) {
        return Err(PublishedRunError::InvalidFacts);
    }
    for (name, output) in &manifest.outputs {
        if !workflow_runtime::valid_output_name(name)
            || output
                .columns
                .iter()
                .any(|column| column.name.is_empty() || column.data_type.trim().is_empty())
        {
            return Err(PublishedRunError::InvalidFacts);
        }
    }
    Ok(())
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
pub(super) enum PublishedRunError {
    #[error("Run {run_id} does not exist in Analysis Session {session_id}")]
    #[diagnostic(help(
        "Use the exact Session ID and Run ID returned by the same successful `kat run`"
    ))]
    NotFound { session_id: String, run_id: String },
    #[error("Run is corrupted")]
    #[diagnostic(help("Re-run the Workflow to publish a complete Run"))]
    CorruptPath(#[source] io::Error),
    #[error("Run is corrupted")]
    #[diagnostic(help("Re-run the Workflow to publish a complete Run"))]
    InvalidLayout,
    #[error("Run is corrupted")]
    #[diagnostic(help("Re-run the Workflow to publish a complete Run"))]
    ReadManifest(#[source] io::Error),
    #[error("Run is corrupted")]
    #[diagnostic(help("Re-run the Workflow to publish a complete Run"))]
    DecodeManifest(#[source] serde_json::Error),
    #[error("Run is corrupted")]
    #[diagnostic(help("Re-run the Workflow to publish a complete Run"))]
    InvalidFacts,
    #[error("Run is corrupted")]
    #[diagnostic(help("Re-run the Workflow to publish a complete Run"))]
    InvalidParquet(#[source] parquet::errors::ParquetError),
    #[error("Run storage could not be enumerated")]
    ReadRuns(#[source] io::Error),
    #[error("Run path cannot be represented as native Unicode")]
    NonUnicodePath,
}

#[cfg(test)]
mod tests {
    use std::{fs::File, sync::Arc};

    use arrow_schema::{DataType, Field, Schema};
    use parquet::arrow::ArrowWriter;

    use super::*;

    fn one_output() -> BTreeMap<String, RunOutputMetadata> {
        BTreeMap::from([(
            "main".to_owned(),
            RunOutputMetadata {
                columns: Vec::new(),
                row_count: 0,
            },
        )])
    }

    fn typed_output(data_type: &str, row_count: u64) -> BTreeMap<String, RunOutputMetadata> {
        BTreeMap::from([(
            "main".to_owned(),
            RunOutputMetadata {
                columns: vec![workflow_runtime::Column {
                    name: "value".to_owned(),
                    data_type: data_type.to_owned(),
                }],
                row_count,
            },
        )])
    }

    fn write_empty_parquet(path: &Path, data_type: DataType) {
        write_empty_parquet_schema(path, vec![Field::new("value", data_type, true)]);
    }

    fn write_empty_parquet_schema(path: &Path, fields: Vec<Field>) {
        let schema = Arc::new(Schema::new(fields));
        ArrowWriter::try_new(File::create(path).unwrap(), schema, None)
            .unwrap()
            .close()
            .unwrap();
    }

    fn publish_test_run(
        store: &crate::session_store::SessionStore,
        session_id: &str,
        run_id: RunId,
        child_runs: Vec<String>,
    ) {
        let mut allocation = match store.create_run_in(session_id, run_id.clone()) {
            Ok(allocation) => allocation,
            Err(_) => panic!("create Run allocation"),
        };
        let outputs = allocation.candidate().join("outputs");
        fs::create_dir(&outputs).unwrap();
        write_empty_parquet(&outputs.join("main.parquet"), DataType::Int64);
        let manifest = RunManifest::new(
            session_id.to_owned(),
            run_id.as_str().to_owned(),
            "test-pack".to_owned(),
            "test-workflow".to_owned(),
            child_runs,
            BTreeMap::new(),
            typed_output("int64", 0),
        );
        fs::write(
            allocation.candidate().join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        allocation.mark_run_published();
    }

    #[test]
    fn candidate_output_gate_accepts_matching_parquet_footer_facts() {
        let candidate = tempfile::tempdir().unwrap();
        let outputs = candidate.path().join("outputs");
        fs::create_dir(&outputs).unwrap();
        write_empty_parquet(&outputs.join("main.parquet"), DataType::Int64);

        validate_candidate_outputs(candidate.path(), &typed_output("int64", 0)).unwrap();
    }

    #[test]
    fn candidate_output_gate_accepts_temporal_and_nested_arrow_types() {
        let candidate = tempfile::tempdir().unwrap();
        let outputs = candidate.path().join("outputs");
        fs::create_dir(&outputs).unwrap();
        let fields = vec![
            Field::new("day", DataType::Date32, true),
            Field::new(
                "recorded_at",
                DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into())),
                true,
            ),
            Field::new(
                "items",
                DataType::List(Arc::new(Field::new("item", DataType::Int64, true))),
                true,
            ),
            Field::new(
                "payload",
                DataType::Struct(vec![Field::new("name", DataType::Utf8, true)].into()),
                true,
            ),
        ];
        write_empty_parquet_schema(&outputs.join("main.parquet"), fields);
        let metadata = BTreeMap::from([(
            "main".to_owned(),
            RunOutputMetadata {
                columns: vec![
                    workflow_runtime::Column {
                        name: "day".to_owned(),
                        data_type: "date32[day]".to_owned(),
                    },
                    workflow_runtime::Column {
                        name: "recorded_at".to_owned(),
                        data_type: "timestamp[ns, tz=UTC]".to_owned(),
                    },
                    workflow_runtime::Column {
                        name: "items".to_owned(),
                        data_type: "list<item: int64>".to_owned(),
                    },
                    workflow_runtime::Column {
                        name: "payload".to_owned(),
                        data_type: "struct<name: string>".to_owned(),
                    },
                ],
                row_count: 0,
            },
        )]);

        validate_candidate_outputs(candidate.path(), &metadata).unwrap();
    }

    #[test]
    fn candidate_output_gate_does_not_reject_an_ordered_dictionary() {
        let candidate = tempfile::tempdir().unwrap();
        let outputs = candidate.path().join("outputs");
        fs::create_dir(&outputs).unwrap();
        write_empty_parquet(
            &outputs.join("main.parquet"),
            DataType::Dictionary(Box::new(DataType::Int32), Box::new(DataType::Utf8)),
        );

        validate_candidate_outputs(
            candidate.path(),
            &typed_output("dictionary<values=string, indices=int32, ordered=1>", 0),
        )
        .unwrap();
    }

    #[test]
    fn candidate_output_gate_accepts_an_ordered_nested_dictionary() {
        let candidate = tempfile::tempdir().unwrap();
        let outputs = candidate.path().join("outputs");
        fs::create_dir(&outputs).unwrap();
        write_empty_parquet(
            &outputs.join("main.parquet"),
            DataType::List(Arc::new(Field::new(
                "item",
                DataType::Dictionary(Box::new(DataType::Int32), Box::new(DataType::Utf8)),
                true,
            ))),
        );

        validate_candidate_outputs(
            candidate.path(),
            &typed_output(
                "list<item: dictionary<values=string, indices=int32, ordered=1>>",
                0,
            ),
        )
        .unwrap();
    }

    #[test]
    fn candidate_output_gate_rejects_wrong_dictionary_index_or_value_types() {
        let candidate = tempfile::tempdir().unwrap();
        let outputs = candidate.path().join("outputs");
        fs::create_dir(&outputs).unwrap();
        write_empty_parquet(
            &outputs.join("main.parquet"),
            DataType::Dictionary(Box::new(DataType::Int32), Box::new(DataType::Utf8)),
        );

        for declared in [
            "dictionary<values=string, indices=int16, ordered=0>",
            "dictionary<values=binary, indices=int32, ordered=0>",
        ] {
            assert!(matches!(
                validate_candidate_outputs(candidate.path(), &typed_output(declared, 0)),
                Err(PublishedRunError::InvalidFacts)
            ));
        }
    }

    #[test]
    fn candidate_output_gate_rejects_a_corrupt_parquet_footer() {
        let candidate = tempfile::tempdir().unwrap();
        let outputs = candidate.path().join("outputs");
        fs::create_dir(&outputs).unwrap();
        fs::write(outputs.join("main.parquet"), b"not parquet").unwrap();

        assert!(matches!(
            validate_candidate_outputs(candidate.path(), &typed_output("int64", 0)),
            Err(PublishedRunError::InvalidParquet(_))
        ));
    }

    #[test]
    fn candidate_output_gate_rejects_a_schema_fact_mismatch() {
        let candidate = tempfile::tempdir().unwrap();
        let outputs = candidate.path().join("outputs");
        fs::create_dir(&outputs).unwrap();
        write_empty_parquet(&outputs.join("main.parquet"), DataType::Int64);

        assert!(matches!(
            validate_candidate_outputs(candidate.path(), &typed_output("string", 0)),
            Err(PublishedRunError::InvalidFacts)
        ));
    }

    #[test]
    fn candidate_output_gate_rejects_a_row_count_fact_mismatch() {
        let candidate = tempfile::tempdir().unwrap();
        let outputs = candidate.path().join("outputs");
        fs::create_dir(&outputs).unwrap();
        write_empty_parquet(&outputs.join("main.parquet"), DataType::Int64);

        assert!(matches!(
            validate_candidate_outputs(candidate.path(), &typed_output("int64", 1)),
            Err(PublishedRunError::InvalidFacts)
        ));
    }

    #[test]
    fn candidate_output_gate_rejects_an_undeclared_direct_entry() {
        let candidate = tempfile::tempdir().unwrap();
        let outputs = candidate.path().join("outputs");
        fs::create_dir(&outputs).unwrap();
        fs::write(outputs.join("main.parquet"), b"declared").unwrap();
        fs::write(outputs.join("extra.parquet"), b"undeclared").unwrap();

        assert!(matches!(
            validate_candidate_outputs(candidate.path(), &one_output()),
            Err(PublishedRunError::InvalidLayout)
        ));
    }

    #[test]
    fn published_resolve_rechecks_the_parquet_footer() {
        let temporary = tempfile::tempdir().unwrap();
        let store = crate::session_store::SessionStore::new(temporary.path());
        let opened = store.create().unwrap();
        let session_id = opened.layout().session_id().as_str().to_owned();
        drop(opened);
        let run_id = RunId::generate();
        let mut allocation = match store.create_run_in(&session_id, run_id.clone()) {
            Ok(allocation) => allocation,
            Err(_) => panic!("create Run allocation"),
        };
        let outputs = allocation.candidate().join("outputs");
        fs::create_dir(&outputs).unwrap();
        write_empty_parquet(&outputs.join("main.parquet"), DataType::Int64);
        let manifest = RunManifest::new(
            session_id,
            run_id.as_str().to_owned(),
            "test-pack".to_owned(),
            "test-workflow".to_owned(),
            Vec::new(),
            BTreeMap::new(),
            typed_output("int64", 0),
        );
        fs::write(
            allocation.candidate().join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        allocation.mark_run_published();
        fs::write(outputs.join("main.parquet"), b"not parquet").unwrap();

        assert!(matches!(
            resolve(allocation.layout(), run_id.as_str()),
            Err(PublishedRunError::InvalidParquet(_))
        ));
    }

    #[test]
    fn resolve_all_accepts_a_published_direct_child() {
        let temporary = tempfile::tempdir().unwrap();
        let store = crate::session_store::SessionStore::new(temporary.path());
        let opened = store.create().unwrap();
        let session_id = opened.layout().session_id().as_str();
        let child_run_id = RunId::generate();
        let parent_run_id = RunId::generate();
        publish_test_run(&store, session_id, child_run_id.clone(), Vec::new());
        publish_test_run(
            &store,
            session_id,
            parent_run_id,
            vec![child_run_id.as_str().to_owned()],
        );

        assert_eq!(resolve_all(opened.layout()).unwrap().len(), 2);
    }

    #[test]
    fn child_completion_adds_published_descendants_omitted_from_the_snapshot() {
        let temporary = tempfile::tempdir().unwrap();
        let store = crate::session_store::SessionStore::new(temporary.path());
        let opened = store.create().unwrap();
        let session_id = opened.layout().session_id().as_str();
        let grandchild_run_id = RunId::generate();
        let child_run_id = RunId::generate();
        let parent_run_id = RunId::generate();
        publish_test_run(&store, session_id, grandchild_run_id.clone(), Vec::new());
        publish_test_run(
            &store,
            session_id,
            child_run_id.clone(),
            vec![grandchild_run_id.as_str().to_owned()],
        );
        publish_test_run(
            &store,
            session_id,
            parent_run_id.clone(),
            vec![child_run_id.as_str().to_owned()],
        );
        let parent = resolve(opened.layout(), parent_run_id.as_str()).unwrap();

        let completed = complete_child_run_references(opened.layout(), vec![parent]).unwrap();
        assert_eq!(
            completed
                .into_iter()
                .map(|run| run.run_id)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                parent_run_id.as_str().to_owned(),
                child_run_id.as_str().to_owned(),
                grandchild_run_id.as_str().to_owned(),
            ])
        );
    }

    #[test]
    fn resolve_rejects_a_self_referencing_child_run() {
        let temporary = tempfile::tempdir().unwrap();
        let store = crate::session_store::SessionStore::new(temporary.path());
        let opened = store.create().unwrap();
        let session_id = opened.layout().session_id().as_str();
        let run_id = RunId::generate();
        publish_test_run(
            &store,
            session_id,
            run_id.clone(),
            vec![run_id.as_str().to_owned()],
        );

        assert!(matches!(
            resolve(opened.layout(), run_id.as_str()),
            Err(PublishedRunError::InvalidFacts)
        ));
    }

    #[test]
    fn resolve_all_rejects_a_self_referencing_child_run() {
        let temporary = tempfile::tempdir().unwrap();
        let store = crate::session_store::SessionStore::new(temporary.path());
        let opened = store.create().unwrap();
        let session_id = opened.layout().session_id().as_str();
        let run_id = RunId::generate();
        publish_test_run(
            &store,
            session_id,
            run_id.clone(),
            vec![run_id.as_str().to_owned()],
        );

        assert!(matches!(
            resolve_all(opened.layout()),
            Err(PublishedRunError::InvalidFacts)
        ));
    }

    #[test]
    fn resolve_all_rejects_a_missing_child_run() {
        let temporary = tempfile::tempdir().unwrap();
        let store = crate::session_store::SessionStore::new(temporary.path());
        let opened = store.create().unwrap();
        let session_id = opened.layout().session_id().as_str();
        let parent_run_id = RunId::generate();
        let missing_child_run_id = RunId::generate();
        publish_test_run(
            &store,
            session_id,
            parent_run_id,
            vec![missing_child_run_id.as_str().to_owned()],
        );

        assert!(matches!(
            resolve_all(opened.layout()),
            Err(PublishedRunError::InvalidFacts)
        ));
    }

    #[test]
    fn resolve_all_rejects_an_unpublished_child_run() {
        let temporary = tempfile::tempdir().unwrap();
        let store = crate::session_store::SessionStore::new(temporary.path());
        let opened = store.create().unwrap();
        let session_id = opened.layout().session_id().as_str();
        let parent_run_id = RunId::generate();
        let unpublished_child_run_id = RunId::generate();
        let _unpublished = match store.create_run_in(session_id, unpublished_child_run_id.clone()) {
            Ok(allocation) => allocation,
            Err(_) => panic!("create unpublished Run allocation"),
        };
        publish_test_run(
            &store,
            session_id,
            parent_run_id,
            vec![unpublished_child_run_id.as_str().to_owned()],
        );

        assert!(matches!(
            resolve_all(opened.layout()),
            Err(PublishedRunError::InvalidFacts)
        ));
    }

    #[test]
    fn unselected_candidate_disappearance_is_ignored() {
        let temporary = tempfile::tempdir().unwrap();
        let missing = temporary.path().join("disappeared-run");

        assert!(
            optional_unselected_candidate(fs::symlink_metadata(&missing))
                .unwrap()
                .is_none()
        );
        assert!(
            optional_unselected_candidate(dunce::canonicalize(&missing))
                .unwrap()
                .is_none()
        );
    }
}
