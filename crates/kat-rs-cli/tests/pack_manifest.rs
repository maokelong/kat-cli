use std::fs;

use kat_rs_cli::trace_runtime::pack::load_pack;
use tempfile::tempdir;

#[test]
fn load_pack_accepts_manifest_and_transform_specs() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();
    fs::create_dir_all(root.join("derived")).expect("derived dir");
    fs::create_dir_all(root.join("rules")).expect("rules dir");
    fs::create_dir_all(root.join("analyses")).expect("analyses dir");
    fs::write(
        root.join("pack.yaml"),
        r#"
id: fixture-core
name: Fixture Core Trace Pack
derived:
  - derived/segments.yaml
rules:
  - rules/thread_identity.yaml
analyses:
  - analyses/critical-path.plan.yaml
"#,
    )
    .expect("pack yaml");
    fs::write(
        root.join("derived/segments.yaml"),
        r#"
id: segments
kind: sql.view
inputs: [thread_state]
sql: queries/segments.sql
output:
  table: derived_output
  schema: segments.v1
safety:
  allowedTables: [thread_state]
"#,
    )
    .expect("transform yaml");
    fs::write(root.join("rules/thread_identity.yaml"), "rules: {}\n").expect("rules yaml");
    fs::write(
        root.join("analyses/critical-path.plan.yaml"),
        "id: fixture.analysis\nrequires: {}\nsteps: []\n",
    )
    .expect("analysis yaml");

    let pack = load_pack(root).expect("pack loads");

    assert_eq!(pack.manifest.id, "fixture-core");
    assert_eq!(pack.transforms.len(), 1);
    assert_eq!(pack.transforms[0].id, "segments");
}

#[test]
fn load_pack_rejects_missing_referenced_file() {
    let dir = tempdir().expect("tempdir");
    fs::write(
        dir.path().join("pack.yaml"),
        "id: fixture-core\nderived: [derived/missing.yaml]\n",
    )
    .expect("pack yaml");

    let error = load_pack(dir.path()).expect_err("missing transform is rejected");

    assert!(
        error.to_string().contains("derived/missing.yaml"),
        "error: {error:#}"
    );
}

#[test]
fn load_pack_rejects_references_that_escape_pack_root() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path().join("pack");
    fs::create_dir_all(root.join("derived")).expect("derived dir");
    fs::write(
        root.join("pack.yaml"),
        "id: fixture-core\nderived: [../outside.yaml]\n",
    )
    .expect("pack yaml");
    fs::write(
        dir.path().join("outside.yaml"),
        "id: outside\nkind: sql.view\ninputs: [thread_state]\noutput:\n  table: x\n  schema: x.v1\n",
    )
    .expect("outside transform yaml");

    let error = load_pack(&root).expect_err("escaping transform is rejected");

    assert!(
        error.to_string().contains("escapes pack root"),
        "error: {error:#}"
    );
}

#[test]
fn load_pack_rejects_duplicate_transform_id() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();
    fs::create_dir_all(root.join("derived")).expect("derived dir");
    fs::write(
        root.join("pack.yaml"),
        "id: fixture-core\nderived: [derived/a.yaml, derived/b.yaml]\n",
    )
    .expect("pack yaml");
    for name in ["a", "b"] {
        fs::write(
            root.join(format!("derived/{name}.yaml")),
            "id: duplicate\nkind: sql.view\ninputs: [thread_state]\noutput:\n  table: x\n  schema: x.v1\n",
        )
        .expect("transform yaml");
    }

    let error = load_pack(root).expect_err("duplicate transform id is rejected");

    assert!(
        error.to_string().contains("duplicate transform id"),
        "error: {error:#}"
    );
    assert!(
        error.to_string().contains("derived/a.yaml"),
        "error: {error:#}"
    );
    assert!(
        error.to_string().contains("derived/b.yaml"),
        "error: {error:#}"
    );
}

#[test]
fn load_pack_rejects_duplicate_analysis_id_with_paths() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();
    fs::create_dir_all(root.join("analyses")).expect("analyses dir");
    fs::write(
        root.join("pack.yaml"),
        "id: fixture-core\nanalyses: [analyses/a.yaml, analyses/b.yaml]\n",
    )
    .expect("pack yaml");
    for name in ["a", "b"] {
        fs::write(
            root.join(format!("analyses/{name}.yaml")),
            "id: duplicate\nrequires: {}\nsteps: []\n",
        )
        .expect("analysis yaml");
    }

    let error = load_pack(root).expect_err("duplicate analysis id is rejected");

    assert!(
        error.to_string().contains("duplicate analysis id"),
        "error: {error:#}"
    );
    assert!(
        error.to_string().contains("analyses/a.yaml"),
        "error: {error:#}"
    );
    assert!(
        error.to_string().contains("analyses/b.yaml"),
        "error: {error:#}"
    );
}
