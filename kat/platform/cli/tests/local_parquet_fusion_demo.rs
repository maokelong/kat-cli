use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[allow(dead_code)]
mod support;

#[allow(dead_code)]
#[path = "support/test_home.rs"]
mod test_home;

fn write_parquet_inputs(python: &Path, root: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let events = root.join("events.parquet");
    let labels = root.join("labels");
    let owners = root.join("owners.parquet");
    fs::create_dir_all(&labels).unwrap();
    let script = r#"
import sys
from pathlib import Path

import pyarrow as pa
import pyarrow.parquet as pq

events, labels, owners = map(Path, sys.argv[1:])
pq.write_table(
    pa.table({
        "event_id": pa.array([1, 2, 3], type=pa.int64()),
        "owner_id": pa.array([10, 20, 20], type=pa.int64()),
        "score": pa.array([5, 15, 25], type=pa.int64()),
    }),
    events,
)
pq.write_table(
    pa.table({
        "event_id": pa.array([1, 2], type=pa.int64()),
        "label": pa.array(["boot", "render"], type=pa.string()),
    }),
    labels / "part-0.parquet",
)
pq.write_table(
    pa.table({
        "event_id": pa.array([3], type=pa.int64()),
        "label": pa.array(["commit"], type=pa.string()),
    }),
    labels / "part-1.parquet",
)
pq.write_table(
    pa.table({
        "owner_id": pa.array([10, 20], type=pa.int64()),
        "owner_name": pa.array(["kernel", "graphics"], type=pa.string()),
    }),
    owners,
)
"#;
    let output = Command::new(python)
        .arg("-c")
        .arg(script)
        .arg(&events)
        .arg(&labels)
        .arg(&owners)
        .output()
        .expect("write Local Parquet Fusion inputs");
    assert!(
        output.status.success(),
        "fixture generation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    (events, labels, owners)
}

#[test]
#[ignore = "requires KAT_TEST_PYTHON and a wheel built from the current checkout"]
fn local_parquet_fusion_demo_runs_the_full_user_loop() {
    let python = PathBuf::from(
        std::env::var_os("KAT_TEST_PYTHON").expect("KAT_TEST_PYTHON identifies CPython"),
    );
    let workflow_wheel = PathBuf::from(
        std::env::var_os("KAT_TEST_WORKFLOW_WHEEL")
            .expect("KAT_TEST_WORKFLOW_WHEEL identifies the current wheel"),
    );
    support::assert_cpython_314(&python);
    let temporary = tempfile::tempdir().unwrap();
    let (_skill, binary) = support::stage_real_host_skill(
        temporary.path(),
        &support::cargo_kat(),
        &python,
        &workflow_wheel,
    );
    let (events, labels, owners) =
        write_parquet_inputs(&support::host_path(&binary), temporary.path());
    let pack = support::repository_path("../../../examples/packs/local-parquet-fusion");

    let mut inspect = Command::new(&binary);
    inspect
        .args(["inspect", "--pack", "local-parquet-fusion", "--pack-dir"])
        .arg(&pack);
    test_home::configure(&mut inspect, temporary.path());
    let inspection = support::response(inspect.output().unwrap());
    assert_eq!(inspection["result"]["name"], "local-parquet-fusion");
    assert_eq!(
        inspection["result"]["workflows"][0]["name"],
        "fuse-local-parquet"
    );

    let mut test = Command::new(&binary);
    test.args(["test", "--pack-dir"]).arg(&pack);
    test_home::configure(&mut test, temporary.path());
    let tested_output = test.output().unwrap();
    assert_eq!(
        tested_output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&tested_output.stderr)
    );
    let tested: serde_json::Value = serde_json::from_slice(&tested_output.stdout).unwrap();
    assert_eq!(tested["status"], "success");
    assert_eq!(tested["result"]["summary"]["passed"], 6);
    assert!(String::from_utf8_lossy(&tested_output.stderr).contains("6 passed"));

    let mut run = Command::new(&binary);
    run.args([
        "run",
        "--pack",
        "local-parquet-fusion",
        "--workflow",
        "fuse-local-parquet",
        "--pack-dir",
    ])
    .arg(&pack)
    .arg("--")
    .arg("--events-path")
    .arg(&events)
    .arg("--labels-path")
    .arg(&labels)
    .arg("--owners-path")
    .arg(&owners)
    .args(["--minimum-score", "10"]);
    test_home::configure(&mut run, temporary.path());
    let ran = support::response(run.output().unwrap());
    assert_eq!(ran["result"]["outputs"]["main"]["row_count"], 2);
    let run_id = ran["result"]["run_id"].as_str().unwrap();

    let sql = r#"
        SELECT event_id, label, owner_name, score
        FROM output.main
        ORDER BY event_id
    "#;
    let mut query = Command::new(&binary);
    query.args(["query", "--run", run_id, "--sql", sql]);
    test_home::configure(&mut query, temporary.path());
    let queried = support::response(query.output().unwrap());
    assert_eq!(
        queried["result"]["rows"],
        serde_json::json!([
            ["2", "render", "graphics", "15"],
            ["3", "commit", "graphics", "25"]
        ])
    );
}
