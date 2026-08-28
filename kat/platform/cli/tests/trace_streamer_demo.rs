use std::{fs, path::PathBuf, process::Command};

#[allow(dead_code)]
mod support;

#[allow(dead_code)]
#[path = "support/test_home.rs"]
mod test_home;

#[test]
#[ignore = "requires KAT_TEST_PYTHON and a wheel built from the current checkout"]
fn trace_streamer_demo_runs_the_full_user_loop() {
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
    let source = temporary.path().join("trace-streamer.db");
    fs::copy(
        support::repository_path("../datasource/tests/fixtures/trace-streamer-thread-cpu-time.db"),
        &source,
    )
    .unwrap();
    let dataset = temporary.path().join("dataset");
    let pack = support::repository_path("../../packs/kat-openharmony-thread-cpu-time");

    let mut import = Command::new(&binary);
    import
        .current_dir(temporary.path())
        .args(["import", "trace-streamer", "--database"])
        .arg(&source)
        .args(["--dataset"])
        .arg(&dataset);
    test_home::configure(&mut import, temporary.path());
    let imported = support::response(import.output().unwrap());
    assert_eq!(
        imported["result"]["path"],
        dunce::canonicalize(&dataset).unwrap().to_str().unwrap()
    );

    let mut inspect_dataset = Command::new(&binary);
    inspect_dataset.args(["inspect", "--dataset"]).arg(&dataset);
    test_home::configure(&mut inspect_dataset, temporary.path());
    let inspection = support::response(inspect_dataset.output().unwrap());
    assert_eq!(
        inspection["result"]["tables"]
            .as_array()
            .unwrap()
            .iter()
            .map(|table| table["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["sched_slice", "thread"]
    );

    let mut inspect_pack = Command::new(&binary);
    inspect_pack
        .args([
            "inspect",
            "--pack",
            "kat-openharmony-thread-cpu-time",
            "--pack-dir",
        ])
        .arg(&pack);
    test_home::configure(&mut inspect_pack, temporary.path());
    let pack_inspection = support::response(inspect_pack.output().unwrap());
    assert_eq!(
        pack_inspection["result"]["workflows"][0]["name"],
        "thread-cpu-time"
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
    assert_eq!(tested["result"]["summary"]["passed"], 5);
    assert!(String::from_utf8_lossy(&tested_output.stderr).contains("5 passed"));

    let mut run = Command::new(&binary);
    run.args([
        "run",
        "--pack",
        "kat-openharmony-thread-cpu-time",
        "--workflow",
        "thread-cpu-time",
        "--pack-dir",
    ])
    .arg(&pack)
    .args(["--dataset"])
    .arg(&dataset);
    test_home::configure(&mut run, temporary.path());
    let ran = support::response(run.output().unwrap());
    assert_eq!(
        ran["result"]["outputs"]["thread_cpu_time_by_cpu"]["row_count"],
        3
    );
    let run_id = ran["result"]["run_id"].as_str().unwrap();

    let sql = r#"
        WITH totals AS (
            SELECT thread_id, thread_name, SUM(observed_cpu_time_ns) AS total_cpu_time_ns
            FROM output.thread_cpu_time_by_cpu
            GROUP BY thread_id, thread_name
        ), primary_cpu AS (
            SELECT
                thread_id,
                thread_name,
                cpu,
                observed_cpu_time_ns,
                ROW_NUMBER() OVER (
                    PARTITION BY thread_id, thread_name
                    ORDER BY observed_cpu_time_ns DESC, cpu ASC
                ) AS cpu_rank
            FROM output.thread_cpu_time_by_cpu
        )
        SELECT totals.thread_id, totals.thread_name, totals.total_cpu_time_ns,
               primary_cpu.cpu, primary_cpu.observed_cpu_time_ns
        FROM totals
        JOIN primary_cpu
          ON totals.thread_id = primary_cpu.thread_id
         AND totals.thread_name = primary_cpu.thread_name
        WHERE primary_cpu.cpu_rank = 1
        ORDER BY totals.total_cpu_time_ns DESC, totals.thread_id ASC, totals.thread_name ASC
        LIMIT 3
    "#;
    let mut query = Command::new(&binary);
    query.args(["query", "--run", run_id, "--sql", sql]);
    test_home::configure(&mut query, temporary.path());
    let queried = support::response(query.output().unwrap());
    assert_eq!(
        queried["result"]["rows"],
        serde_json::json!([
            [15381, "OS_FFRT_0_0", "61999000", 3, "61999000"],
            [15040, ".tencent.wechat", "59973000", 10, "59973000"],
            [2424, "SaInit0", "4878000", 4, "4878000"]
        ])
    );
}
