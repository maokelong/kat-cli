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
