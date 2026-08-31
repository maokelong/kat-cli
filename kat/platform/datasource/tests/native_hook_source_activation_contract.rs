use std::{collections::BTreeSet, fs, path::Path};

use prost::Message;
use tempfile::tempdir;

#[path = "native_hook_source_contract/fixture.rs"]
mod native_hook_fixture;
use native_hook_fixture::{
    full_native_hook_batches, full_native_hook_config, full_native_hook_relation_names,
    profiler_section,
};

#[allow(dead_code)]
mod proto {
    pub mod kat {
        pub mod hitrace {
            include!(concat!(env!("OUT_DIR"), "/kat.hitrace.rs"));
        }

        pub mod native_hook {
            include!(concat!(env!("OUT_DIR"), "/kat.native_hook.rs"));
        }
    }
}

use proto::kat::hitrace::ProfilerPluginData;

#[test]
fn decode_publishes_native_hook_descriptor_and_clock_relations_flat() {
    let root = tempdir().expect("temporary decode directory is created");
    let source = root.path().join("full-native-hook-topology.htrace");
    let destination = root.path().join("relations");
    let (first_batch, second_batch) = full_native_hook_batches();
    let config = full_native_hook_config("boot");
    fs::write(
        &source,
        profiler_section([
            profiler_envelope("nativehook", 21, 7, first_batch.encode_to_vec()),
            profiler_envelope("hookdaemon", 22, 7, second_batch.encode_to_vec()),
            profiler_envelope("nativehook_config", 23, 7, config.clone().encode_to_vec()),
            profiler_envelope("hookdaemon_config", 24, 7, config.encode_to_vec()),
            profiler_envelope("nativehook-preview", 25, 7, vec![0xff]),
            profiler_envelope("future-plugin", 26, 7, vec![0x80]),
        ]),
    )
    .expect("full typed OHOSPROF fixture is written");

    let report = kat_datasource::decode_hitrace(&source, &destination)
        .expect("decode publishes Native Hook relations");

    assert_eq!(
        report.unsupported_plugins(),
        ["future-plugin", "nativehook-preview"]
    );
    assert_eq!(flat_relation_names(&destination), expected_relation_names());
    assert!(
        fs::read_dir(&destination)
            .expect("relation root can be listed")
            .all(|entry| entry
                .expect("relation entry can be read")
                .path()
                .extension()
                .is_some_and(|extension| extension == "parquet"))
    );
}

fn flat_relation_names(root: &Path) -> BTreeSet<String> {
    fs::read_dir(root)
        .expect("flat relation root can be listed")
        .map(|entry| {
            entry
                .expect("relation entry can be read")
                .path()
                .file_stem()
                .and_then(|name| name.to_str())
                .expect("relation name is Unicode")
                .to_owned()
        })
        .collect()
}

fn expected_relation_names() -> BTreeSet<String> {
    let mut names = full_native_hook_relation_names()
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    names.extend(["clock_domain".to_owned(), "clock_snapshot".to_owned()]);
    names
}

fn profiler_envelope(name: &str, status: u32, clock_id: i32, data: Vec<u8>) -> ProfilerPluginData {
    ProfilerPluginData {
        name: name.to_owned(),
        status,
        data,
        clock_id,
        tv_sec: 100 + u64::from(status),
        tv_nsec: 200 + u64::from(status),
        version: format!("route-{status}"),
        sample_interval: status,
    }
}
