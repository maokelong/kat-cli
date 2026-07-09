use std::path::PathBuf;

use kat_rs_cli::python_worker::{PackRunRequest, parse_params};
use serde_json::json;

#[test]
fn pack_run_request_serializes_cli_contract() {
    let request = PackRunRequest {
        pack_root: PathBuf::from("packs/openharmony-critical-path"),
        workflow: "wechat_first_frame_critical_path".to_string(),
        dataset_path: PathBuf::from("dataset"),
        run_dir: PathBuf::from("run"),
        inputs: parse_params(&[
            "root_itid=405".to_string(),
            "max_depth=8".to_string(),
            "app_name=.tencent.wechat".to_string(),
        ])
        .expect("params parse"),
    };

    let value = serde_json::to_value(&request).expect("request serializes");

    assert_eq!(
        value,
        json!({
            "packRoot": "packs/openharmony-critical-path",
            "workflow": "wechat_first_frame_critical_path",
            "datasetPath": "dataset",
            "runDir": "run",
            "inputs": {
                "root_itid": 405,
                "max_depth": 8,
                "app_name": ".tencent.wechat"
            }
        })
    );
}

#[test]
fn pack_run_params_reject_missing_equals() {
    let error = parse_params(&["root_itid".to_string()]).expect_err("invalid param rejected");
    assert!(error.to_string().contains("expected key=value"));
}

#[test]
#[ignore = "requires local Python with datafusion installed"]
fn pack_run_real_python_smoke_is_available_for_manual_verification() {
    assert!(
        std::env::var_os("KAT_RS_PYTHON").is_some(),
        "set KAT_RS_PYTHON to the Python executable used for pack run verification"
    );
}

#[test]
#[ignore = "requires KAT_RS_E2E_DB and local Python with datafusion installed"]
fn local_test_db_e2e_contract_is_documented() {
    let db = std::env::var("KAT_RS_E2E_DB")
        .expect("set KAT_RS_E2E_DB to the local test.db path");
    assert!(
        std::path::Path::new(&db).exists(),
        "KAT_RS_E2E_DB path must exist: {db}"
    );
}
