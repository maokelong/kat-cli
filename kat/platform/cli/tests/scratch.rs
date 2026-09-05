use std::{fs, path::Path, process::Command};

#[allow(dead_code)]
mod support;

fn invoke(binary: &Path, home: &Path, args: &[&str]) -> serde_json::Value {
    let output = Command::new(binary)
        .args(args)
        .env("KAT_DATA_HOME", home)
        .output()
        .unwrap();
    let response: serde_json::Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&output.stderr)));
    assert_eq!(
        output.status.success(),
        response["status"] == "success",
        "{response}"
    );
    response
}

#[test]
#[ignore = "requires KAT_TEST_PYTHON and a wheel built from the current checkout"]
fn scratch_lifecycle_uses_rust_for_direct_nested_and_test_runs() {
    let python = std::env::var_os("KAT_TEST_PYTHON").unwrap();
    let wheel = std::env::var_os("KAT_TEST_WORKFLOW_WHEEL").unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let home = root.join("data-home");
    fs::create_dir(&home).unwrap();
    let (_, binary) = support::stage_real_host_skill(
        root,
        &support::cargo_kat(),
        Path::new(&python),
        Path::new(&wheel),
    );
    let pack = root.join("pack");
    fs::create_dir_all(pack.join("workflows")).unwrap();
    fs::write(pack.join("pack.toml"), "name = 'scratch'\ntitle = 'Scratch'\ndescription = 'Scratch verification'\nowner = 'Test'\n").unwrap();
    fs::write(
        pack.join("workflows/child.py"),
        include_str!("fixtures/scratch_workflows.py"),
    )
    .unwrap();
    fs::write(
        pack.join("workflows/parent.py"),
        include_str!("fixtures/scratch_parent.py"),
    )
    .unwrap();
    fs::write(
        pack.join("scratch_helpers.py"),
        include_str!("fixtures/scratch_helpers.py"),
    )
    .unwrap();
    let evidence = root.join("evidence");
    fs::create_dir(&evidence).unwrap();
    let session = invoke(&binary, &home, &["session", "create"])["result"]["session_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut primary_errors = std::collections::BTreeMap::new();
    for workflow in ["child", "parent"] {
        for mode in [
            "table",
            "none",
            "empty",
            "missing",
            "missing_access",
            "file",
            "business",
            "business_file",
            "output",
            "output_file",
            "parent_file",
            "catch",
        ] {
            if workflow == "child" && ["parent_file", "catch"].contains(&mode) {
                continue;
            }
            let response = invoke(
                &binary,
                &home,
                &[
                    "run",
                    "--session",
                    &session,
                    "--pack",
                    "scratch",
                    "--workflow",
                    workflow,
                    "--pack-dir",
                    pack.to_str().unwrap(),
                    "--",
                    "--mode",
                    mode,
                    "--evidence",
                    evidence.to_str().unwrap(),
                ],
            );
            let success = ["table", "none", "empty", "missing", "catch"].contains(&mode);
            assert_eq!(
                response["status"] == "success",
                success,
                "{workflow}/{mode}: {response}"
            );
            let log = fs::read_to_string(response["log_path"].as_str().unwrap()).unwrap();
            assert!(log.contains("scratch_cleanup:"), "{log}");
            if workflow == "child" {
                if ["business", "output"].contains(&mode) {
                    primary_errors.insert(mode, response["error"].clone());
                }
                if let Some(primary) = mode
                    .strip_suffix("_file")
                    .and_then(|key| primary_errors.get(key))
                {
                    assert_eq!(&response["error"], primary);
                    assert!(log.contains("scratch_cleanup: failure"), "{log}");
                }
                if mode == "table" || mode == "empty" {
                    let run = response["result"]["run_id"].as_str().unwrap();
                    let query = invoke(
                        &binary,
                        &home,
                        &[
                            "query",
                            "--session",
                            &session,
                            "--run",
                            run,
                            "--sql",
                            "SELECT * FROM output.main",
                        ],
                    );
                    assert_eq!(query["status"], "success", "{query}");
                    let rows =
                        fs::read_to_string(query["result"]["path"].as_str().unwrap()).unwrap();
                    assert_eq!(
                        rows.trim(),
                        if mode == "table" { "{\"value\":7}" } else { "" }
                    );
                }
            }
            let mut manifests = std::collections::BTreeMap::new();
            for entry in fs::read_dir(&evidence).unwrap() {
                let path = entry.unwrap().path();
                let captured: serde_json::Value =
                    serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                let scratch = Path::new(captured["scratch"].as_str().unwrap());
                assert!(
                    fs::symlink_metadata(scratch).is_err(),
                    "{workflow}/{mode}: {scratch:?}"
                );
                let candidate = scratch
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .join("runs")
                    .join(scratch.file_name().unwrap());
                let published = success && !(mode == "catch" && path.ends_with("child.json"))
                    || mode == "parent_file" && path.ends_with("child.json");
                assert_eq!(
                    candidate.join("manifest.json").is_file(),
                    published,
                    "{workflow}/{mode}: {candidate:?}"
                );
                if published {
                    let manifest: serde_json::Value =
                        serde_json::from_slice(&fs::read(candidate.join("manifest.json")).unwrap())
                            .unwrap();
                    manifests.insert(
                        path.file_stem().unwrap().to_str().unwrap().to_owned(),
                        manifest,
                    );
                }
                fs::remove_file(path).unwrap();
            }
            if let Some(parent) = manifests.get("parent") {
                let expected = manifests
                    .get("child")
                    .map(|child| vec![child["run_id"].clone()])
                    .unwrap_or_default();
                assert_eq!(parent["child_runs"], serde_json::json!(expected));
            }
        }
    }
    fs::create_dir(pack.join("tests")).unwrap();
    fs::write(
        pack.join("tests/test_scratch.py"),
        include_str!("fixtures/scratch_pack_test.py"),
    )
    .unwrap();
    let response = invoke(
        &binary,
        &home,
        &["test", "--pack-dir", pack.to_str().unwrap()],
    );
    let log = fs::read_to_string(response["log_path"].as_str().unwrap()).unwrap();
    assert_eq!(response["status"], "success", "{response}\n{log}");
}
