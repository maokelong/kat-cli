use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackRunRequest {
    pub pack_root: PathBuf,
    pub workflow: String,
    pub dataset_path: PathBuf,
    pub run_dir: PathBuf,
    pub inputs: BTreeMap<String, Value>,
}

pub fn parse_params(params: &[String]) -> Result<BTreeMap<String, Value>> {
    let mut values = BTreeMap::new();
    for param in params {
        let Some((key, value)) = param.split_once('=') else {
            bail!("expected key=value parameter, got {param:?}");
        };
        if key.is_empty() {
            bail!("parameter key must not be empty");
        }
        values.insert(key.to_string(), parse_value(value));
    }
    Ok(values)
}

fn parse_value(value: &str) -> Value {
    if value.eq_ignore_ascii_case("true") {
        json!(true)
    } else if value.eq_ignore_ascii_case("false") {
        json!(false)
    } else if let Ok(integer) = value.parse::<i64>() {
        json!(integer)
    } else if let Ok(float) = value.parse::<f64>() {
        json!(float)
    } else {
        json!(value)
    }
}

pub fn run_discovery(pack_root: &Path) -> Result<String> {
    let output = base_python_command()
        .arg("-m")
        .arg("kat_runtime.worker.discovery")
        .arg("--pack-root")
        .arg(pack_root)
        .output()
        .context("failed to start Python discovery worker")?;
    command_stdout(output, "Python discovery worker")
}

pub fn run_pack(request: &PackRunRequest) -> Result<String> {
    fs::create_dir_all(&request.run_dir).with_context(|| {
        format!(
            "failed to create run directory {}",
            request.run_dir.display()
        )
    })?;
    let request_path = request.run_dir.join("request.json");
    let request_json =
        serde_json::to_vec_pretty(request).context("failed to serialize pack run request")?;
    fs::write(&request_path, request_json).with_context(|| {
        format!(
            "failed to write pack run request {}",
            request_path.display()
        )
    })?;

    let output = base_python_command()
        .arg("-m")
        .arg("kat_runtime.worker.run")
        .arg("--request")
        .arg(&request_path)
        .output()
        .context("failed to start Python run worker")?;
    command_stdout(output, "Python run worker")
}

fn base_python_command() -> Command {
    let python = env::var_os("KAT_RS_PYTHON").unwrap_or_else(|| "python".into());
    let mut command = Command::new(python);
    command.env("PYTHONPATH", pythonpath());
    command
}

fn pythonpath() -> String {
    let root = env!("CARGO_MANIFEST_DIR");
    let repo_root = Path::new(root)
        .parent()
        .and_then(Path::parent)
        .expect("crate is under crates/kat-rs-cli");
    let separator = if cfg!(windows) { ";" } else { ":" };
    format!(
        "{}{}{}",
        repo_root.join("python").join("kat-python-sdk").display(),
        separator,
        repo_root
            .join("python")
            .join("kat-python-runtime")
            .display()
    )
}

fn command_stdout(output: std::process::Output, label: &str) -> Result<String> {
    if output.status.success() {
        String::from_utf8(output.stdout).context("Python worker stdout was not UTF-8")
    } else {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow!(
            "{label} failed with status {}\nstdout:\n{stdout}\nstderr:\n{stderr}",
            output.status
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        path::{Path, PathBuf},
        sync::Mutex,
    };

    use tempfile::tempdir;

    use super::{PackRunRequest, run_discovery, run_pack};
    use serde_json::{Value, json};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn run_discovery_uses_configured_python_and_sets_pythonpath() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let dir = tempdir().expect("tempdir");
        let capture = dir.path().join("capture.json");
        let script = fake_python_script(dir.path(), "discovery");

        set_python_env(&script);
        let stdout = run_discovery(dir.path()).expect("discovery runs");

        assert_eq!(stdout.trim_end(), "discovery ok");
        let payload: Value = serde_json::from_slice(&fs::read(&capture).expect("capture exists"))
            .expect("capture json");
        assert_eq!(
            payload["args"],
            json!([
                "-m",
                "kat_runtime.worker.discovery",
                "--pack-root",
                dir.path().display().to_string()
            ])
        );
        let pythonpath = payload["pythonpath"].as_str().expect("pythonpath str");
        assert!(
            pythonpath.contains("python\\kat-python-sdk")
                || pythonpath.contains("python/kat-python-sdk")
        );
        assert!(
            pythonpath.contains("python\\kat-python-runtime")
                || pythonpath.contains("python/kat-python-runtime")
        );
    }

    #[test]
    fn run_pack_writes_request_and_invokes_run_worker() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let dir = tempdir().expect("tempdir");
        let capture = dir.path().join("capture.json");
        let run_dir = dir.path().join("run");
        let script = fake_python_script(dir.path(), "run");

        set_python_env(&script);
        let request = PackRunRequest {
            pack_root: dir.path().join("pack"),
            workflow: "wf".to_string(),
            dataset_path: dir.path().join("dataset"),
            run_dir: run_dir.clone(),
            inputs: super::parse_params(&[
                "flag=true".to_string(),
                "count=8".to_string(),
                "name=hello".to_string(),
            ])
            .expect("params parse"),
        };

        let stdout = run_pack(&request).expect("run worker runs");

        assert_eq!(stdout.trim_end(), "run ok");
        let payload: Value = serde_json::from_slice(&fs::read(&capture).expect("capture exists"))
            .expect("capture json");
        let request_path = run_dir.join("request.json");
        assert_eq!(
            payload["args"],
            json!([
                "-m",
                "kat_runtime.worker.run",
                "--request",
                request_path.display().to_string()
            ])
        );
        let written: Value =
            serde_json::from_slice(&fs::read(request_path).expect("request json exists"))
                .expect("request json");
        assert_eq!(written["workflow"], json!("wf"));
        assert_eq!(written["inputs"]["flag"], json!(true));
        assert_eq!(written["inputs"]["count"], json!(8));
        assert_eq!(written["inputs"]["name"], json!("hello"));
    }

    fn set_python_env(script: &Path) {
        unsafe {
            env::set_var("KAT_RS_PYTHON", script.as_os_str());
        }
    }

    fn fake_python_script(dir: &Path, _mode: &str) -> PathBuf {
        let capture = dir.join("capture.json");
        let ps1 = dir.join("fake-python.ps1");
        let cmd = dir.join("fake-python.cmd");

        fs::write(
            &ps1,
            format!(
                "$payload = @{{ args = $args; pythonpath = $env:PYTHONPATH }} | ConvertTo-Json -Compress\nSet-Content -LiteralPath '{}' -Value $payload\nif ($args.Length -gt 1 -and $args[1] -eq 'kat_runtime.worker.discovery') {{ Write-Output 'discovery ok' }} else {{ Write-Output 'run ok' }}\n",
                capture.display()
            ),
        )
        .expect("ps1 written");
        fs::write(
            &cmd,
            format!(
                "@echo off\r\npowershell -NoProfile -ExecutionPolicy Bypass -File \"{}\" %*\r\n",
                ps1.display()
            ),
        )
        .expect("cmd written");
        cmd
    }
}
