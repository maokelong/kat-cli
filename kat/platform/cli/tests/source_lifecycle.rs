use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

#[allow(dead_code)]
mod support;

#[allow(dead_code)]
#[path = "support/test_home.rs"]
mod test_home;

const PACK_NAME: &str = "kat-kernel";
const SOURCE_NAME: &str = "raw_smaps";
const HITRACE_SOURCE_NAME: &str = "hitrace";
const WORKFLOW_NAME: &str = "process-memory-summary";

fn repository_path(relative: &str) -> PathBuf {
    dunce::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)).unwrap()
}

fn configured_command(binary: &Path, root: &Path) -> Command {
    let mut command = Command::new(binary);
    command.current_dir(root);
    test_home::configure(&mut command, root);
    command
}

fn assert_fields(value: &serde_json::Value, expected: &[&str]) {
    let actual = value
        .as_object()
        .expect("JSON object")
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);
}

fn successful(output: Output, artifacts: &[&str]) -> serde_json::Value {
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let mut fields = vec!["result", "status"];
    fields.extend_from_slice(artifacts);
    assert_fields(&response, &fields);
    assert_eq!(response["status"], "success");
    for artifact in artifacts {
        assert!(
            Path::new(response[*artifact].as_str().expect("artifact path")).is_file(),
            "missing {artifact}"
        );
    }
    response
}

fn failed(output: Output) -> serde_json::Value {
    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_fields(&response, &["error", "log_path", "status"]);
    assert_eq!(response["status"], "failure");
    assert!(response["error"]["message"].is_string());
    assert!(Path::new(response["log_path"].as_str().unwrap()).is_file());
    response
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
        "Source lifecycle E2E requires CPython 3.14"
    );
}

fn install_private_wheel(python: &Path, wheel: &Path) {
    let output = Command::new(python)
        .args([
            "-m",
            "pip",
            "install",
            "--disable-pip-version-check",
            "--no-deps",
            "--no-index",
        ])
        .arg(wheel)
        .output()
        .expect("install native wheel into the staged Workflow Host");
    assert!(
        output.status.success(),
        "native wheel installation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_header_only_hitrace(path: &Path) {
    let mut header = vec![0_u8; 1024];
    header[0..8].copy_from_slice(&0x464F_5250_534F_484F_u64.to_le_bytes());
    header[8..16].copy_from_slice(&1024_u64.to_le_bytes());
    for (offset, value) in [60, 68, 76, 84, 92, 100].into_iter().zip(1_u64..=6) {
        header[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    fs::write(path, header).unwrap();
}

fn write_observer_pack(root: &Path) -> PathBuf {
    let pack = root.join("observer-pack");
    fs::create_dir_all(pack.join("workflows")).unwrap();
    fs::write(
        pack.join("pack.toml"),
        "name = \"observer\"\ntitle = \"Observer\"\ndescription = \"Lazy Source fixture\"\nowner = \"Test\"\n",
    )
    .unwrap();
    fs::write(
        pack.join("workflows/no_source.py"),
        r#"import kat

@kat.workflow(name="no-source", title="No Source")
def no_source(ctx: kat.Context):
    """Return one row without touching any Dataset Source."""
    return ctx.sql("SELECT 1 AS value")
"#,
    )
    .unwrap();
    pack
}

fn bind_source(
    binary: &Path,
    root: &Path,
    pack: &Path,
    dataset: &Path,
    file: &str,
    replace: bool,
) -> Output {
    let mut command = configured_command(binary, root);
    command
        .args([
            "bind",
            "--pack",
            PACK_NAME,
            "--source",
            SOURCE_NAME,
            "--dataset",
        ])
        .arg(dataset)
        .arg("--pack-dir")
        .arg(pack);
    if replace {
        command.arg("--replace");
    }
    command.args(["--", "--files", file]).output().unwrap()
}

fn run_workflow(
    binary: &Path,
    root: &Path,
    pack_name: &str,
    workflow: &str,
    pack: &Path,
    dataset: &Path,
) -> Output {
    let mut command = configured_command(binary, root);
    command
        .args(["run", "--pack", pack_name, "--workflow", workflow])
        .arg("--pack-dir")
        .arg(pack)
        .arg("--dataset")
        .arg(dataset)
        .output()
        .unwrap()
}

fn assert_external_result(response: &serde_json::Value, dataset_path: &str) {
    assert_eq!(
        response["result"],
        serde_json::json!({
            "path": dataset_path,
            "pack": PACK_NAME,
            "source": SOURCE_NAME,
            "kind": "external"
        })
    );
}

#[test]
#[ignore = "requires KAT_TEST_PYTHON and a wheel built from the current checkout"]
fn source_lifecycle_uses_real_installed_workflow_host_end_to_end() {
    let python = PathBuf::from(
        std::env::var_os("KAT_TEST_PYTHON").expect("KAT_TEST_PYTHON identifies CPython"),
    );
    let workflow_wheel = PathBuf::from(
        std::env::var_os("KAT_TEST_WORKFLOW_WHEEL")
            .expect("KAT_TEST_WORKFLOW_WHEEL identifies the current wheel"),
    );
    assert_cpython_314(&python);

    let temporary = tempfile::tempdir().unwrap();
    let (_skill, binary) = support::stage_real_host_skill(
        temporary.path(),
        &support::cargo_kat(),
        &python,
        &workflow_wheel,
    );
    let pack = repository_path("../../packs/kat-kernel");
    let observer_pack = write_observer_pack(temporary.path());
    let fixtures = pack.join("tests/fixtures");
    let capture = temporary.path().join("capture.smaps");
    let corrupt = temporary.path().join("corrupt.smaps");
    fs::copy(fixtures.join("snapshot-a.smaps"), &capture).unwrap();
    fs::copy(fixtures.join("corrupt-metric.smaps"), &corrupt).unwrap();
    let capture_path = dunce::canonicalize(&capture).unwrap();
    let dataset = temporary.path().join("dataset");
    let subset_dataset = temporary.path().join("subset-dataset");

    let mut inspect_pack = configured_command(&binary, temporary.path());
    inspect_pack
        .args(["inspect", "--pack", PACK_NAME, "--pack-dir"])
        .arg(&pack);
    let inspected_pack = successful(inspect_pack.output().unwrap(), &["log_path"]);
    assert_eq!(
        inspected_pack["result"],
        serde_json::json!({
            "name": PACK_NAME,
            "title": "Kernel Performance",
            "description": "Analyze captured kernel data through reusable source facts and workflows.",
            "owner": "Kernel Team",
            "source_guide": fs::read_to_string(pack.join("SOURCES.md")).unwrap(),
            "sources": [
                {
                    "name": HITRACE_SOURCE_NAME,
                    "parameters": [{
                        "name": "trace",
                        "option": "--trace",
                        "type": "path",
                        "required": true
                    }]
                },
                {
                    "name": SOURCE_NAME,
                    "parameters": [{
                        "name": "files",
                        "option": "--files",
                        "type": "path",
                        "required": true,
                        "repeatable": true
                    }]
                }
            ],
            "workflows": [{
                "name": WORKFLOW_NAME,
                "title": "Process Memory by Pathname",
                "description": "按 SMAPS 快照汇总各 pathname 的常驻内存与按比例分摊内存。",
                "parameters": []
            }]
        })
    );

    let missing_binding = successful(
        bind_source(
            &binary,
            temporary.path(),
            &pack,
            &dataset,
            "missing.smaps",
            false,
        ),
        &["log_path"],
    );
    let dataset_path = dunce::canonicalize(&dataset)
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert_external_result(&missing_binding, &dataset_path);

    let mut inspect_external = configured_command(&binary, temporary.path());
    inspect_external
        .args(["inspect", "--dataset"])
        .arg(&dataset);
    let inspected_external = successful(inspect_external.output().unwrap(), &[]);
    assert_eq!(
        inspected_external["result"],
        serde_json::json!({
            "path": dataset_path,
            "sources": [{
                "pack": PACK_NAME,
                "source": SOURCE_NAME,
                "kind": "external"
            }]
        })
    );

    // 未引用 Source 的 Workflow 必须在无效 External Binding 存在时仍可完成。
    let observer_run = successful(
        run_workflow(
            &binary,
            temporary.path(),
            "observer",
            "no-source",
            &observer_pack,
            &dataset,
        ),
        &["log_path"],
    );
    assert_fields(&observer_run["result"], &["outputs", "run_id"]);
    assert_fields(
        &observer_run["result"]["outputs"]["main"],
        &["columns", "row_count"],
    );
    assert_eq!(observer_run["result"]["outputs"]["main"]["row_count"], 1);

    let missing_run = failed(run_workflow(
        &binary,
        temporary.path(),
        PACK_NAME,
        WORKFLOW_NAME,
        &pack,
        &dataset,
    ));
    assert!(missing_run["error"].to_string().contains("missing.smaps"));

    let corrupt_binding = successful(
        bind_source(
            &binary,
            temporary.path(),
            &pack,
            &dataset,
            "corrupt.smaps",
            true,
        ),
        &["log_path"],
    );
    assert_external_result(&corrupt_binding, &dataset_path);
    let corrupt_run = failed(run_workflow(
        &binary,
        temporary.path(),
        PACK_NAME,
        WORKFLOW_NAME,
        &pack,
        &dataset,
    ));
    assert!(corrupt_run["error"].to_string().contains("corrupt.smaps"));

    let valid_binding = successful(
        bind_source(
            &binary,
            temporary.path(),
            &pack,
            &dataset,
            "capture.smaps",
            true,
        ),
        &["log_path"],
    );
    assert_external_result(&valid_binding, &dataset_path);

    let valid_run = successful(
        run_workflow(
            &binary,
            temporary.path(),
            PACK_NAME,
            WORKFLOW_NAME,
            &pack,
            &dataset,
        ),
        &["log_path"],
    );
    assert_fields(&valid_run["result"], &["outputs", "run_id"]);
    assert_eq!(
        valid_run["result"]["outputs"]["process_memory_by_pathname"]["row_count"],
        2
    );

    let mut query_external = configured_command(&binary, temporary.path());
    query_external
        .args(["query", "--dataset"])
        .arg(&dataset)
        .arg("--pack-dir")
        .arg(&pack)
        .args([
            "--sql",
            r#"SELECT COUNT(*) AS count FROM "kat-kernel".raw_smaps.mappings"#,
        ]);
    let queried_external = successful(query_external.output().unwrap(), &["log_path"]);
    assert_eq!(
        queried_external["result"]["rows"],
        serde_json::json!([["3"]])
    );

    let mut materialize_all = configured_command(&binary, temporary.path());
    materialize_all
        .args([
            "materialize",
            "--pack",
            PACK_NAME,
            "--source",
            SOURCE_NAME,
            "--dataset",
        ])
        .arg(&dataset)
        .arg("--replace")
        .arg("--pack-dir")
        .arg(&pack);
    let materialized_all = successful(materialize_all.output().unwrap(), &["log_path"]);
    assert_eq!(
        materialized_all["result"],
        serde_json::json!({
            "path": dataset_path,
            "pack": PACK_NAME,
            "source": SOURCE_NAME,
            "kind": "materialized",
            "tables": ["mappings", "snapshots"]
        })
    );
    let materialized_tables = dataset.join("sources/kat-kernel/raw_smaps/tables");
    let mut table_files = fs::read_dir(&materialized_tables)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            assert!(entry.file_type().unwrap().is_file());
            entry.file_name().to_string_lossy().into_owned()
        })
        .collect::<Vec<_>>();
    table_files.sort();
    assert_eq!(table_files, ["mappings.parquet", "snapshots.parquet"]);
    let bindings: serde_json::Value =
        serde_json::from_slice(&fs::read(dataset.join("bindings.json")).unwrap()).unwrap();
    assert_eq!(
        bindings["bindings"][0]["arguments"],
        serde_json::json!(["--files", "capture.smaps"])
    );
    assert_eq!(
        bindings["bindings"][0]["working_directory"],
        dunce::canonicalize(temporary.path())
            .unwrap()
            .to_str()
            .unwrap()
    );

    let mut redo_all = configured_command(&binary, temporary.path());
    redo_all
        .args([
            "materialize",
            "--pack",
            PACK_NAME,
            "--source",
            SOURCE_NAME,
            "--dataset",
        ])
        .arg(&dataset)
        .arg("--replace")
        .arg("--pack-dir")
        .arg(&pack);
    let redone_all = successful(redo_all.output().unwrap(), &["log_path"]);
    assert_eq!(
        redone_all["result"]["tables"],
        serde_json::json!(["mappings", "snapshots"])
    );

    let mut inspect_materialized = configured_command(&binary, temporary.path());
    inspect_materialized
        .args(["inspect", "--dataset"])
        .arg(&dataset);
    let inspected_materialized = successful(inspect_materialized.output().unwrap(), &[]);
    assert_fields(&inspected_materialized["result"], &["path", "sources"]);
    let source = &inspected_materialized["result"]["sources"][0];
    assert_fields(source, &["kind", "pack", "source", "tables"]);
    assert_eq!(source["pack"], PACK_NAME);
    assert_eq!(source["source"], SOURCE_NAME);
    assert_eq!(source["kind"], "materialized");
    let tables = source["tables"].as_array().unwrap();
    assert_eq!(
        tables
            .iter()
            .map(|table| table["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["mappings", "snapshots"]
    );
    for table in tables {
        assert_fields(table, &["columns", "name"]);
        for column in table["columns"].as_array().unwrap() {
            assert_fields(column, &["name", "nullable", "type"]);
            assert_eq!(column["nullable"], false);
        }
    }
    assert_eq!(
        tables[0]["columns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|column| column["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "snapshot_id",
            "start_address",
            "end_address",
            "permissions",
            "offset",
            "device",
            "inode",
            "pathname",
            "size_kib",
            "rss_kib",
            "pss_kib"
        ]
    );
    assert_eq!(
        tables[1]["columns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|column| column["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["snapshot_id", "source_file"]
    );

    let mut materialize_subset = configured_command(&binary, temporary.path());
    materialize_subset
        .args([
            "materialize",
            "--pack",
            PACK_NAME,
            "--source",
            SOURCE_NAME,
            "--dataset",
        ])
        .arg(&subset_dataset)
        .args(["--table", "snapshots", "--pack-dir"])
        .arg(&pack)
        .args(["--", "--files", "capture.smaps"]);
    let materialized_subset = successful(materialize_subset.output().unwrap(), &["log_path"]);
    let subset_dataset_path = dunce::canonicalize(&subset_dataset)
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        materialized_subset["result"],
        serde_json::json!({
            "path": subset_dataset_path,
            "pack": PACK_NAME,
            "source": SOURCE_NAME,
            "kind": "materialized",
            "tables": ["snapshots"]
        })
    );

    let mut inspect_subset = configured_command(&binary, temporary.path());
    inspect_subset
        .args(["inspect", "--dataset"])
        .arg(&subset_dataset);
    let inspected_subset = successful(inspect_subset.output().unwrap(), &[]);
    assert_eq!(
        inspected_subset["result"]["sources"][0]["tables"]
            .as_array()
            .unwrap()
            .iter()
            .map(|table| table["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["snapshots"]
    );

    let mut query_missing_partial_table = configured_command(&binary, temporary.path());
    query_missing_partial_table
        .args(["query", "--dataset"])
        .arg(&subset_dataset)
        .arg("--pack-dir")
        .arg(&pack)
        .args([
            "--sql",
            r#"SELECT COUNT(*) FROM "kat-kernel".raw_smaps.mappings"#,
        ]);
    let partial_failure = failed(query_missing_partial_table.output().unwrap());
    assert!(partial_failure["error"].to_string().contains("mappings"));

    // REDO 重放保存的 Source recipe；不保存旧的 --table 选择，所以省略时恢复全表。
    let mut redo_subset = configured_command(&binary, temporary.path());
    redo_subset
        .args([
            "materialize",
            "--pack",
            PACK_NAME,
            "--source",
            SOURCE_NAME,
            "--dataset",
        ])
        .arg(&subset_dataset)
        .arg("--replace")
        .arg("--pack-dir")
        .arg(&pack);
    let redone_subset = successful(redo_subset.output().unwrap(), &["log_path"]);
    assert_eq!(
        redone_subset["result"]["tables"],
        serde_json::json!(["mappings", "snapshots"])
    );

    fs::remove_file(&capture).unwrap();
    let sql = r#"
        SELECT snapshots.snapshot_id, snapshots.source_file,
               COUNT(mappings.start_address) AS mapping_count
        FROM "kat-kernel".raw_smaps.snapshots AS snapshots
        JOIN "kat-kernel".raw_smaps.mappings AS mappings
          ON snapshots.snapshot_id = mappings.snapshot_id
        GROUP BY snapshots.snapshot_id, snapshots.source_file
        ORDER BY snapshots.snapshot_id
    "#;
    let mut query = configured_command(&binary, temporary.path());
    query
        .args(["query", "--dataset"])
        .arg(&dataset)
        .args(["--sql", sql]);
    let queried = successful(query.output().unwrap(), &["log_path"]);
    assert_fields(&queried["result"], &["columns", "dataset", "rows"]);
    assert_eq!(
        queried["result"]["dataset"],
        serde_json::json!({"status": "available", "path": dataset_path})
    );
    assert_eq!(
        queried["result"]["columns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|column| {
                assert_fields(column, &["name", "type"]);
                column["name"].as_str().unwrap()
            })
            .collect::<Vec<_>>(),
        ["snapshot_id", "source_file", "mapping_count"]
    );
    assert_eq!(
        queried["result"]["rows"],
        serde_json::json!([["0", capture_path.to_str().unwrap(), "3"]])
    );

    let mut test = configured_command(&binary, temporary.path());
    test.args(["test", "--pack-dir"]).arg(&pack).args([
        "--test",
        "tests/test_process_memory.py::test_kat_run_uses_real_source_arguments",
    ]);
    let tested = successful(test.output().unwrap(), &["log_path", "test_report_path"]);
    assert_eq!(
        tested["result"],
        serde_json::json!({"summary": {"passed": 1}})
    );
}

#[test]
#[ignore = "requires KAT_TEST_PYTHON and wheels built from the current checkout"]
fn hitrace_source_uses_official_ffi_across_external_and_materialized_queries() {
    let python = PathBuf::from(
        std::env::var_os("KAT_TEST_PYTHON").expect("KAT_TEST_PYTHON identifies CPython"),
    );
    let workflow_wheel = PathBuf::from(
        std::env::var_os("KAT_TEST_WORKFLOW_WHEEL")
            .expect("KAT_TEST_WORKFLOW_WHEEL identifies the current wheel"),
    );
    let hitrace_wheel = PathBuf::from(
        std::env::var_os("KAT_TEST_HITRACE_WHEEL")
            .expect("KAT_TEST_HITRACE_WHEEL identifies the current native wheel"),
    );
    assert_cpython_314(&python);

    let temporary = tempfile::tempdir().unwrap();
    let (_skill, binary) = support::stage_real_host_skill(
        temporary.path(),
        &support::cargo_kat(),
        &python,
        &workflow_wheel,
    );
    install_private_wheel(&support::host_path(&binary), &hitrace_wheel);

    let pack = repository_path("../../packs/kat-kernel");
    let dataset = temporary.path().join("hitrace-dataset");
    let trace = temporary.path().join("capture.htrace");
    write_header_only_hitrace(&trace);

    let mut bind = configured_command(&binary, temporary.path());
    bind.args([
        "bind",
        "--pack",
        PACK_NAME,
        "--source",
        HITRACE_SOURCE_NAME,
        "--dataset",
    ])
    .arg(&dataset)
    .arg("--pack-dir")
    .arg(&pack)
    .args(["--", "--trace", "capture.htrace"]);
    let bound = successful(bind.output().unwrap(), &["log_path"]);
    assert_eq!(bound["result"]["kind"], "external");

    let sql = r#"SELECT COUNT(*) AS rows FROM "kat-kernel".hitrace.clock_domain"#;
    let mut query_external = configured_command(&binary, temporary.path());
    query_external
        .args(["query", "--dataset"])
        .arg(&dataset)
        .arg("--pack-dir")
        .arg(&pack)
        .args(["--sql", sql]);
    let external = successful(query_external.output().unwrap(), &["log_path"]);
    assert_eq!(external["result"]["rows"], serde_json::json!([["6"]]));

    let mut materialize = configured_command(&binary, temporary.path());
    materialize
        .args([
            "materialize",
            "--pack",
            PACK_NAME,
            "--source",
            HITRACE_SOURCE_NAME,
            "--dataset",
        ])
        .arg(&dataset)
        .arg("--replace")
        .arg("--pack-dir")
        .arg(&pack);
    let materialized = successful(materialize.output().unwrap(), &["log_path"]);
    assert_eq!(
        materialized["result"]["tables"],
        serde_json::json!(["clock_domain", "clock_snapshot"])
    );

    fs::remove_file(&trace).unwrap();
    let mut query_materialized = configured_command(&binary, temporary.path());
    query_materialized
        .args(["query", "--dataset"])
        .arg(&dataset)
        .args(["--sql", sql]);
    let materialized_query = successful(query_materialized.output().unwrap(), &["log_path"]);
    assert_eq!(
        materialized_query["result"]["rows"],
        serde_json::json!([["6"]])
    );
}
