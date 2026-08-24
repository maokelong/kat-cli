use prost::Message;
use std::{fs, io};
use tempfile::tempdir;

const PROFILER_HEADER_SIZE: usize = 1024;
const PROFILER_HEADER_MAGIC: u64 = 0x464F_5250_534F_484F;
const HIPROFILER_PROTOBUF_BIN: u32 = 0;

#[test]
fn hitrace_staging_rejects_profiler_frames_without_a_hitrace_header() {
    let root = tempdir().expect("tempdir");
    let trace_path = root.path().join("frame-only.hitrace");
    let mut bytes = Vec::new();
    append_segment(
        &mut bytes,
        TestProfilerPluginData {
            name: "ftrace-plugin".to_owned(),
            status: 0,
            data: vec![1, 2, 3],
            clock_id: 2,
            tv_sec: 10,
            tv_nsec: 100,
            version: "1.0".to_owned(),
            sample_interval: 8,
        },
    );
    fs::write(&trace_path, bytes).expect("trace is written");

    let error = kat_datasource::stage_hitrace(&trace_path, root.path().join("dataset"), |_| Ok(()))
        .expect_err("frame-only input is rejected");

    let diagnostic = format!("{error:?}");
    assert!(
        diagnostic.contains("missing OHOSPROF header"),
        "{diagnostic}"
    );
}

#[test]
fn hitrace_staging_rejects_overflowing_section_length_without_panicking() {
    let root = tempdir().expect("tempdir");
    let trace_path = root.path().join("overflowing-section.hitrace");
    let mut bytes = profiler_section(Vec::new());
    bytes.extend_from_slice(&overflowing_section_header());
    fs::write(&trace_path, bytes).expect("trace is written");

    let error = kat_datasource::stage_hitrace(&trace_path, root.path().join("dataset"), |_| Ok(()))
        .expect_err("overflowing section length is rejected");

    let diagnostic = format!("{error:?}");
    assert!(
        diagnostic.contains("invalid profiler section length"),
        "{diagnostic}"
    );
}

#[test]
fn hitrace_staging_reuses_migrated_tables_and_reports_unknown_content() {
    let root = tempdir().expect("tempdir");
    let trace_path = root.path().join("capture.hitrace");
    let target = root.path().join("dataset");
    let mut bytes = profiler_section(vec![
        TestProfilerPluginData {
            name: "z-plugin".to_string(),
            status: 0,
            data: vec![1],
            clock_id: 0,
            tv_sec: 0,
            tv_nsec: 0,
            version: String::new(),
            sample_interval: 0,
        },
        TestProfilerPluginData {
            name: "a-plugin_config".to_string(),
            status: 0,
            data: vec![2],
            clock_id: 0,
            tv_sec: 0,
            tv_nsec: 0,
            version: String::new(),
            sample_interval: 0,
        },
        TestProfilerPluginData {
            name: "z-plugin".to_string(),
            status: 0,
            data: vec![3],
            clock_id: 0,
            tv_sec: 0,
            tv_nsec: 0,
            version: String::new(),
            sample_interval: 0,
        },
    ]);
    bytes.extend(profiler_section_body(1000, Vec::new()));
    bytes.extend(profiler_section_body(77, Vec::new()));
    fs::write(&trace_path, bytes).expect("trace is written");

    let mut unsupported_content = Vec::new();
    let staged = kat_datasource::stage_hitrace(&trace_path, &target, |content| {
        unsupported_content.push((
            content.kind().to_owned(),
            content.value().to_owned(),
            content.byte_offset(),
        ));
        Ok(())
    })
    .expect("Hitrace staging succeeds");

    assert_eq!(staged.unsupported_plugins(), ["a-plugin", "z-plugin"]);
    assert_eq!(staged.unsupported_section_types(), [77, 1000]);
    assert_eq!(
        unsupported_content
            .iter()
            .map(|(kind, value, _)| (kind.as_str(), value.as_str()))
            .collect::<Vec<_>>(),
        [
            ("plugin", "z-plugin"),
            ("plugin", "a-plugin"),
            ("plugin", "z-plugin"),
            ("section_type", "1000"),
            ("section_type", "77"),
        ]
    );
    assert!(
        unsupported_content
            .windows(2)
            .all(|content| content[0].2 < content[1].2)
    );
    assert_eq!(staged.table_names(), ["clock_domain", "clock_snapshot"]);
    assert!(staged.tables_directory().is_dir());
    assert!(!target.join(".kat-dataset").exists());
}

#[test]
fn hitrace_staging_streams_repeated_unknown_occurrences_without_retaining_them() {
    let root = tempdir().expect("tempdir");
    let trace_path = root.path().join("many-unknown.hitrace");
    let target = root.path().join("dataset");
    let frames = (0..=8192)
        .map(|index| TestProfilerPluginData {
            name: "future-plugin".to_owned(),
            status: 0,
            data: vec![(index % 255) as u8],
            clock_id: 0,
            tv_sec: 0,
            tv_nsec: 0,
            version: String::new(),
            sample_interval: 0,
        })
        .collect();
    fs::write(&trace_path, profiler_section(frames)).expect("trace is written");

    let mut observed = 0;
    let staged = kat_datasource::stage_hitrace(&trace_path, &target, |_| {
        observed += 1;
        Ok(())
    })
    .expect("unknown occurrences remain stageable");

    assert_eq!(observed, 8193);
    assert_eq!(staged.unsupported_plugins(), ["future-plugin"]);
}

#[test]
fn hitrace_staging_streams_unknown_occurrences_before_decode_failure() {
    let root = tempdir().expect("tempdir");
    let trace_path = root.path().join("partially-invalid.hitrace");
    let mut bytes = profiler_section(vec![
        TestProfilerPluginData {
            name: "first-plugin".to_owned(),
            status: 0,
            data: vec![1],
            clock_id: 0,
            tv_sec: 0,
            tv_nsec: 0,
            version: String::new(),
            sample_interval: 0,
        },
        TestProfilerPluginData {
            name: "second-plugin_config".to_owned(),
            status: 0,
            data: vec![2],
            clock_id: 0,
            tv_sec: 0,
            tv_nsec: 0,
            version: String::new(),
            sample_interval: 0,
        },
    ]);
    bytes.extend_from_slice(b"truncated-section");
    fs::write(&trace_path, bytes).expect("trace is written");

    let mut observed = Vec::new();
    kat_datasource::stage_hitrace(&trace_path, root.path().join("dataset"), |content| {
        observed.push(content.value().to_owned());
        Ok(())
    })
    .expect_err("truncated capture is rejected");

    assert_eq!(observed, ["first-plugin", "second-plugin"]);
}

#[test]
fn unsupported_content_observer_failure_precedes_staging_target_inspection() {
    let root = tempdir().expect("tempdir");
    let trace_path = root.path().join("capture.hitrace");
    let target = root.path().join("dataset");
    fs::write(
        &trace_path,
        profiler_section(vec![TestProfilerPluginData {
            name: "future-plugin".to_owned(),
            status: 0,
            data: vec![1],
            clock_id: 0,
            tv_sec: 0,
            tv_nsec: 0,
            version: String::new(),
            sample_interval: 0,
        }]),
    )
    .expect("trace is written");
    fs::create_dir(&target).expect("target exists");
    fs::write(target.join("sentinel"), "unchanged").expect("sentinel is written");

    let error = kat_datasource::stage_hitrace(&trace_path, &target, |_| {
        Err(io::Error::new(io::ErrorKind::WriteZero, "log is full"))
    })
    .expect_err("observer failure rejects staging");

    assert!(matches!(
        error,
        kat_datasource::HitraceStagingError::ObserveUnsupportedContent { .. }
    ));
    assert_eq!(
        fs::read_to_string(target.join("sentinel")).expect("sentinel remains readable"),
        "unchanged"
    );
}

#[test]
fn invalid_hitrace_preserves_a_nonempty_staging_target() {
    let root = tempdir().expect("tempdir");
    let trace_path = root.path().join("invalid.hitrace");
    let target = root.path().join("dataset");
    fs::write(&trace_path, b"not a Hitrace capture").expect("invalid trace is written");
    fs::create_dir(&target).expect("target directory is created");
    fs::write(target.join("sentinel"), "unchanged").expect("sentinel is written");

    kat_datasource::stage_hitrace(&trace_path, &target, |_| Ok(()))
        .expect_err("invalid Hitrace is rejected");

    assert_eq!(
        fs::read_to_string(target.join("sentinel")).expect("sentinel remains readable"),
        "unchanged"
    );
}

#[derive(Clone, PartialEq, Message)]
struct TestProfilerPluginData {
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

fn profiler_section(plugins: Vec<TestProfilerPluginData>) -> Vec<u8> {
    let mut body = Vec::new();
    for plugin in plugins {
        append_segment(&mut body, plugin);
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

fn append_segment(bytes: &mut Vec<u8>, plugin: TestProfilerPluginData) {
    let segment = plugin.encode_to_vec();
    bytes.extend_from_slice(&(segment.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&segment);
}

fn overflowing_section_header() -> Vec<u8> {
    let mut bytes = vec![0; PROFILER_HEADER_SIZE];
    bytes[0..8].copy_from_slice(&PROFILER_HEADER_MAGIC.to_le_bytes());
    bytes[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
    bytes[56..60].copy_from_slice(&HIPROFILER_PROTOBUF_BIN.to_le_bytes());
    bytes
}
