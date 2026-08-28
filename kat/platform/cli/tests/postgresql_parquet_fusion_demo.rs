use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

#[allow(dead_code)]
mod support;

#[allow(dead_code)]
#[path = "support/test_home.rs"]
mod test_home;

fn required_environment(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required by the PostgreSQL E2E"))
}

fn response(output: Output, secret: &str) -> serde_json::Value {
    assert_secret_absent("command stdout", &output.stdout, secret);
    assert_secret_absent("command stderr", &output.stderr, secret);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "success");
    response
}

fn assert_secret_absent(label: &str, contents: &[u8], secret: &str) {
    assert!(
        !String::from_utf8_lossy(contents).contains(secret),
        "{label} exposed the PostgreSQL test secret"
    );
}

fn assert_tree_has_no_secret(root: &Path, secret: &str) {
    if !root.exists() {
        return;
    }
    for entry in fs::read_dir(root).expect("read KAT Data Home") {
        let path = entry.expect("read KAT Data Home entry").path();
        if path.is_dir() {
            assert_tree_has_no_secret(&path, secret);
        } else {
            assert_secret_absent(
                path.to_string_lossy().as_ref(),
                &fs::read(&path).expect("read KAT artifact"),
                secret,
            );
        }
    }
}

fn repository_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(relative)
        .canonicalize()
        .unwrap()
}

fn assert_cpython_314(python: &Path) {
    let output = Command::new(python)
        .args([
            "-c",
            "import sys; print(f'{sys.implementation.name} {sys.version_info.major}.{sys.version_info.minor}')",
        ])
        .output()
        .expect("inspect Workflow Host Python");
    assert!(
        output.status.success(),
        "Workflow Host Python inspection failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "cpython 3.14",
        "PostgreSQL E2E requires CPython 3.14"
    );
}

fn write_trace_fixture(python: &Path, trace_root: &Path) {
    let script = r#"
from pathlib import Path
import sys
import pyarrow as pa
import pyarrow.parquet as pq

root = Path(sys.argv[1])
root.mkdir()
pq.write_table(
    pa.table({
        "cpu": pa.array([0, 0, 0, 0, 1, 1, 1, 2, 2], type=pa.int64()),
        "next_thread_id": pa.array([101, 102, 0, 0, 0, 102, 0, 103, 0], type=pa.int64()),
        "timestamp": pa.array([100, 140, 190, 220, 180, 200, 240, 160, 180], type=pa.int64()),
    }),
    root / "sched_switch.parquet",
)
"#;
    let output = Command::new(python)
        .args(["-I", "-B", "-c", script])
        .arg(trace_root)
        .output()
        .expect("write local Parquet fixture");
    assert!(
        output.status.success(),
        "local Parquet fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_poison_probe_pack(pack: &Path) {
    let datasource_root = pack.join("helpers").join("datasources");
    let workflow_root = pack.join("workflows");
    fs::create_dir_all(&datasource_root).expect("create poison probe datasource directory");
    fs::create_dir(&workflow_root).expect("create poison probe workflow directory");
    fs::copy(
        repository_path(
            "../../../examples/packs/postgresql-parquet-fusion/helpers/datasources/postgresql.py",
        ),
        datasource_root.join("postgresql.py"),
    )
    .expect("copy production PostgreSQL executor into poison probe");
    fs::write(
        pack.join("pack.toml"),
        r#"name = "postgresql-poison-probe"
title = "PostgreSQL Poison Probe"
description = "Exercises a caught PostgreSQL failure through the public CLI."
owner = "KAT Contributors"
"#,
    )
    .expect("write poison probe PACK declaration");
    fs::write(
        workflow_root.join("probe.py"),
        r#"import kat

from kat.pack.helpers.datasources import postgresql


@kat.workflow(
    name="probe",
    title="Probe PostgreSQL failure",
    required_tables=[],
    parameters={"profile": "service", "database": "database"},
)
def probe(ctx: kat.Context, profile: str, database: str):
    """Catch a real source failure; the poisoned Context must reject publishing."""
    try:
        postgresql.provider(
            ctx,
            profile=profile,
            database=database,
        ).query("SELECT 1::BIGINT AS value", name="failed")
    except RuntimeError:
        pass
    return None
"#,
    )
    .expect("write poison probe Workflow");
}

fn assert_real_authentication_failure_is_sanitized(
    binary: &Path,
    root: &Path,
    profile: &str,
    database: &str,
    actual_secret: &str,
) {
    const INVALID_SECRET: &str = "kat-invalid-password-sentinel";
    let pack = root.join("postgresql-poison-probe");
    write_poison_probe_pack(&pack);

    let mut command = Command::new(binary);
    command
        .args([
            "run",
            "--pack",
            "postgresql-poison-probe",
            "--workflow",
            "probe",
            "--pack-dir",
        ])
        .arg(&pack)
        .arg("--")
        .args(["--profile", profile, "--database", database])
        .env("PGPASSWORD", INVALID_SECRET);
    test_home::configure(&mut command, root);
    let output = command
        .output()
        .expect("run real authentication failure probe");

    for secret in [actual_secret, INVALID_SECRET] {
        assert_secret_absent("failed run stdout", &output.stdout, secret);
        assert_secret_absent("failed run stderr", &output.stderr, secret);
    }
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "failure");
    assert!(response.get("result").is_none());
    assert!(
        response["error"]
            .to_string()
            .contains("PostgreSQL query failed")
    );

    let operation_log =
        fs::read(response["log_path"].as_str().unwrap()).expect("read failed run operation log");
    let data_home = test_home::data_home(root);
    for secret in [actual_secret, INVALID_SECRET] {
        assert_secret_absent("failed run operation log", &operation_log, secret);
        assert_tree_has_no_secret(&data_home, secret);
    }
    let runs = data_home.join("runs");
    assert!(
        !runs.exists() || fs::read_dir(runs).unwrap().next().is_none(),
        "a caught Provider failure must not publish a Run candidate"
    );
}

#[test]
#[ignore = "requires a real Workflow Host wheel and external libpq test services"]
fn postgresql_parquet_fusion_demo_runs_the_full_user_loop() {
    let python = PathBuf::from(
        std::env::var_os("KAT_TEST_PYTHON").expect("KAT_TEST_PYTHON identifies CPython"),
    );
    let workflow_wheel = PathBuf::from(
        std::env::var_os("KAT_TEST_WORKFLOW_WHEEL")
            .expect("KAT_TEST_WORKFLOW_WHEEL identifies the current wheel"),
    );
    let readonly_profile = required_environment("KAT_TEST_POSTGRES_READONLY_PROFILE");
    let writer_profile = required_environment("KAT_TEST_POSTGRES_WRITER_PROFILE");
    let telemetry_database = required_environment("KAT_TEST_POSTGRES_TELEMETRY_DATABASE");
    let control_database = required_environment("KAT_TEST_POSTGRES_CONTROL_DATABASE");
    let secret = required_environment("KAT_TEST_POSTGRES_SECRET_SENTINEL");
    let service_file = PathBuf::from(required_environment("PGSERVICEFILE"));
    let password_file = PathBuf::from(required_environment("PGPASSFILE"));
    assert_ne!(telemetry_database, control_database);
    let services = fs::read_to_string(service_file).expect("read PostgreSQL service file");
    assert!(services.contains(&format!("[{readonly_profile}]")));
    assert!(services.contains(&format!("[{writer_profile}]")));
    let passwords = fs::read_to_string(password_file).expect("read PostgreSQL password file");
    assert!(
        passwords.contains(&secret),
        "PostgreSQL secret sentinel must be the password file credential"
    );
    assert_cpython_314(&python);

    let temporary = tempfile::tempdir().unwrap();
    let (_skill, binary) = support::stage_real_host_skill(
        temporary.path(),
        &support::cargo_kat(),
        &python,
        &workflow_wheel,
    );
    let pack = repository_path("../../../examples/packs/postgresql-parquet-fusion");
    let trace_root = temporary.path().join("trace");
    write_trace_fixture(&python, &trace_root);

    let mut inspect = Command::new(&binary);
    inspect
        .args([
            "inspect",
            "--pack",
            "postgresql-parquet-fusion",
            "--pack-dir",
        ])
        .arg(&pack)
        .env_remove("PGSERVICEFILE")
        .env_remove("PGPASSFILE");
    test_home::configure(&mut inspect, temporary.path());
    let inspected = response(inspect.output().unwrap(), &secret);
    assert_eq!(
        inspected["result"]["workflows"][0]["name"],
        "fuse-observations"
    );
    assert_eq!(
        inspected["result"]["workflows"][0]["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|parameter| parameter["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "profile",
            "telemetry_database",
            "control_database",
            "trace_root",
            "start_ns",
            "end_ns"
        ]
    );

    let mut test = Command::new(&binary);
    test.args(["test", "--pack-dir"]).arg(&pack);
    test_home::configure(&mut test, temporary.path());
    let tested_output = test.output().unwrap();
    assert_secret_absent("kat test stdout", &tested_output.stdout, &secret);
    assert_secret_absent("kat test stderr", &tested_output.stderr, &secret);
    assert_eq!(
        tested_output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&tested_output.stderr)
    );
    let tested: serde_json::Value = serde_json::from_slice(&tested_output.stdout).unwrap();
    assert_eq!(tested["status"], "success");
    assert_eq!(tested["result"]["summary"]["passed"], 32);
    assert!(String::from_utf8_lossy(&tested_output.stderr).contains("32 passed"));

    let mut run = Command::new(&binary);
    run.args([
        "run",
        "--pack",
        "postgresql-parquet-fusion",
        "--workflow",
        "fuse-observations",
        "--pack-dir",
    ])
    .arg(&pack)
    .arg("--")
    .args(["--profile", &readonly_profile])
    .args(["--telemetry-database", &telemetry_database])
    .args(["--control-database", &control_database])
    .arg("--trace-root")
    .arg(&trace_root)
    .args(["--start-ns", "100", "--end-ns", "220"]);
    test_home::configure(&mut run, temporary.path());
    let ran = response(run.output().unwrap(), &secret);
    assert_eq!(ran["result"]["outputs"].as_object().unwrap().len(), 1);
    assert_eq!(ran["result"]["outputs"]["main"]["row_count"], 3);
    assert_eq!(
        ran["result"]["outputs"]["main"]["columns"],
        serde_json::json!([
            {"name":"thread_id","type":"int64"},
            {"name":"process_id","type":"int64"},
            {"name":"process_name","type":"string"},
            {"name":"observed_at","type":"int64"},
            {"name":"cpu","type":"int64"},
            {"name":"run_start","type":"int64"},
            {"name":"run_end","type":"int64"},
            {"name":"cpu_usage","type":"double"}
        ])
    );
    let run_id = ran["result"]["run_id"].as_str().unwrap();
    let run_root = test_home::data_home(temporary.path())
        .join("runs")
        .join(run_id);
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(run_root.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(
        manifest["inputs"],
        serde_json::json!({
            "profile": readonly_profile,
            "telemetry_database": telemetry_database,
            "control_database": control_database,
            "trace_root": trace_root.to_string_lossy(),
            "start_ns": "100",
            "end_ns": "220"
        })
    );
    assert_eq!(manifest["outputs"].as_object().unwrap().len(), 1);
    assert!(manifest["outputs"].get("main").is_some());
    let output_root = run_root.join("outputs");
    assert!(output_root.join("main.parquet").is_file());
    assert_eq!(
        fs::read_dir(&output_root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>(),
        ["main.parquet"]
    );

    let mut query = Command::new(&binary);
    query.args([
        "query",
        "--run",
        run_id,
        "--sql",
        "SELECT thread_id, process_id, process_name, observed_at, cpu, run_start, run_end, cpu_usage FROM output.main ORDER BY observed_at, thread_id",
    ]);
    test_home::configure(&mut query, temporary.path());
    let queried = response(query.output().unwrap(), &secret);
    assert_eq!(
        queried["result"]["rows"],
        serde_json::json!([
            ["101", "10", "renderer", "100", "0", "100", "140", 0.25],
            ["102", "20", "system-server", "150", "0", "140", "190", 0.5],
            ["102", "20", "system-server", "200", "1", "200", "240", 0.75]
        ])
    );

    let failure_root = temporary.path().join("failure");
    fs::create_dir(&failure_root).unwrap();
    assert_real_authentication_failure_is_sanitized(
        &binary,
        &failure_root,
        &readonly_profile,
        &telemetry_database,
        &secret,
    );

    assert_tree_has_no_secret(&test_home::data_home(temporary.path()), &secret);
}
