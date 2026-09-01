use std::{fs, path::Path};

use prost::Message;
use tempfile::tempdir;

const PROFILER_HEADER_SIZE: usize = 1024;
const PROFILER_HEADER_MAGIC: u64 = 0x464F_5250_534F_484F;
const HIPROFILER_PROTOBUF_BIN: u32 = 0;

#[derive(Clone, PartialEq, Message)]
struct ProfilerPluginData {
    #[prost(string, tag = "1")]
    name: String,
    #[prost(uint32, tag = "2")]
    status: u32,
    #[prost(bytes = "vec", tag = "3")]
    data: Vec<u8>,
    #[prost(int32, tag = "4")]
    clock_id: i32,
    #[prost(uint64, tag = "5")]
    tv_sec: u64,
    #[prost(uint64, tag = "6")]
    tv_nsec: u64,
    #[prost(string, tag = "7")]
    version: String,
    #[prost(uint32, tag = "8")]
    sample_interval: u32,
}

#[test]
fn existing_destination_is_rejected_before_source_is_read() {
    let root = tempdir().expect("tempdir");
    let source = root.path().join("missing.htrace");
    let destination = root.path().join("relations");
    fs::create_dir(&destination).expect("destination exists");
    let sentinel = destination.join("owned-by-caller");
    fs::write(&sentinel, b"keep").expect("sentinel is written");

    let error = kat_datasource::decode_hitrace(&source, &destination)
        .expect_err("existing destination is rejected");

    assert!(
        error.to_string().contains("destination already exists"),
        "unexpected error: {error:#}"
    );
    assert_eq!(fs::read(&sentinel).expect("sentinel remains"), b"keep");
}

#[test]
fn decode_publishes_flat_relations_and_sorted_unsupported_report() {
    let root = tempdir().expect("tempdir");
    let source = root.path().join("unknown-content.htrace");
    let destination = root.path().join("relations");
    let mut bytes = profiler_section(["zeta", "alpha_config", "zeta"]);
    bytes.extend(profiler_section_body(1000, Vec::new()));
    bytes.extend(profiler_section_body(77, Vec::new()));
    bytes.extend(profiler_section_body(1000, Vec::new()));
    fs::write(&source, bytes).expect("fixture is written");

    let report =
        kat_datasource::decode_hitrace(&source, &destination).expect("Hitrace decode succeeds");

    assert_eq!(report.unsupported_plugins(), ["alpha", "zeta"]);
    assert_eq!(report.unsupported_section_types(), [77, 1000]);
    assert_eq!(
        relation_names(&destination),
        ["clock_domain.parquet", "clock_snapshot.parquet"]
    );
}

#[test]
fn corrupt_source_leaves_neither_destination_nor_staging() {
    let root = tempdir().expect("tempdir");
    let source = root.path().join("corrupt.htrace");
    let destination = root.path().join("relations");
    fs::write(&source, b"not a Hitrace file").expect("corrupt fixture is written");

    let error = kat_datasource::decode_hitrace(&source, &destination)
        .expect_err("corrupt Hitrace is rejected");

    assert!(
        error.to_string().contains("failed to decode hitrace file"),
        "unexpected error: {error:#}"
    );
    assert!(!destination.exists());
    assert!(
        fs::read_dir(root.path())
            .expect("parent can be listed")
            .all(|entry| !entry
                .expect("parent entry can be read")
                .file_name()
                .to_string_lossy()
                .starts_with(".kat-datasource-staging-"))
    );
}

fn profiler_section(names: impl IntoIterator<Item = &'static str>) -> Vec<u8> {
    let mut body = Vec::new();
    for name in names {
        let envelope = ProfilerPluginData {
            name: name.to_owned(),
            ..Default::default()
        }
        .encode_to_vec();
        body.extend_from_slice(&(envelope.len() as u32).to_le_bytes());
        body.extend_from_slice(&envelope);
    }
    profiler_section_body(HIPROFILER_PROTOBUF_BIN, body)
}

fn profiler_section_body(data_type: u32, body: Vec<u8>) -> Vec<u8> {
    let mut bytes = vec![0; PROFILER_HEADER_SIZE];
    bytes[0..8].copy_from_slice(&PROFILER_HEADER_MAGIC.to_le_bytes());
    bytes[8..16].copy_from_slice(&((PROFILER_HEADER_SIZE + body.len()) as u64).to_le_bytes());
    bytes[56..60].copy_from_slice(&data_type.to_le_bytes());
    bytes.extend_from_slice(&body);
    bytes
}

fn relation_names(destination: &Path) -> Vec<String> {
    let mut names = fs::read_dir(destination)
        .expect("destination can be listed")
        .map(|entry| {
            entry
                .expect("destination entry can be read")
                .file_name()
                .into_string()
                .expect("relation name is Unicode")
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}
