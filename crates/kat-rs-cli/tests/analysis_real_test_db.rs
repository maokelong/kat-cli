use std::path::{Path, PathBuf};

use kat_rs_cli::trace_runtime::{
    analysis::runner::{AnalysisRunConfig, run_analysis},
    pack::load_pack,
};
use serde_json::json;
use tempfile::tempdir;

#[test]
#[ignore = "requires local test/test.db"]
fn openharmony_critical_path_finds_wechat_first_draw_in_test_db() {
    let root = workspace_root();
    let raw_db = local_test_db_path(&root);
    assert!(raw_db.is_file(), "missing {}", raw_db.display());
    let dir = tempdir().expect("tempdir");
    let pack = load_pack(root.join("packs/openharmony-core")).expect("pack");

    let run_dir = run_analysis(AnalysisRunConfig {
        raw_db,
        scratch_db: dir.path().join("scratch.db"),
        run_root: dir.path().join("runs"),
        run_id: "wechat-first-draw".to_string(),
        pack,
        analysis_id: "openharmony.critical_path".to_string(),
        params: json!({
            "target_process": ".tencent.wechat",
            "marker": "firstDrawFrame:1"
        }),
    })
    .expect("analysis run");

    let state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(run_dir.join("state.json")).expect("state"))
            .expect("state json");
    assert_eq!(state["root"]["itid"], json!(405));
    assert_eq!(state["root"]["vsync_id"], json!(3269));
    assert_eq!(state["root"]["start_ts"], json!(246307034375i64));
    assert_eq!(state["root"]["end_ts"], json!(246329389063i64));

    let evidence = std::fs::read_to_string(run_dir.join("evidence.jsonl")).expect("evidence");
    assert!(evidence.contains("self_execution"));

    let report = std::fs::read_to_string(run_dir.join("report.md")).expect("report");
    assert!(report.contains(".tencent.wechat"));
    assert!(report.contains("CreateImagePixelMap resource:///1140850711.png"));
    assert!(report.contains("# Facts"));
    assert!(report.contains("# Inferences"));
    assert!(report.contains("# Uncertainty"));
}

fn local_test_db_path(root: &Path) -> PathBuf {
    if let Some(path) = std::env::var_os("KAT_RS_TEST_DB") {
        return PathBuf::from(path);
    }

    let worktree_db = root.join("test/test.db");
    if worktree_db.is_file() {
        return worktree_db;
    }

    if root
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == ".worktrees")
    {
        if let Some(repo_root) = root.parent().and_then(Path::parent) {
            return repo_root.join("test/test.db");
        }
    }

    worktree_db
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}
