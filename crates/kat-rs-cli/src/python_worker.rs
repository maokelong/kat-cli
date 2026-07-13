use std::{
    collections::BTreeMap,
    env, fs,
    path::{Component, Path, PathBuf},
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
    ensure_run_dir_outside_dataset(&request.dataset_path, &request.run_dir)?;
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
    Command::new(python)
}

fn ensure_run_dir_outside_dataset(dataset_path: &Path, run_dir: &Path) -> Result<()> {
    let dataset_root = dunce::canonicalize(dataset_path).with_context(|| {
        format!(
            "failed to resolve dataset directory {}",
            dataset_path.display()
        )
    })?;
    let run_dir = normalized_absolute(run_dir)
        .with_context(|| format!("failed to resolve run directory {}", run_dir.display()))?;

    if is_same_or_child_path(&run_dir, &dataset_root) {
        bail!(
            "pack run directory must be outside dataset directory: {}",
            run_dir.display()
        );
    }

    Ok(())
}

fn normalized_absolute(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()?.join(path)
    };
    Ok(normalize_components(absolute))
}

fn normalize_components(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(value) => normalized.push(value),
            Component::RootDir | Component::Prefix(_) => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

#[cfg(windows)]
fn is_same_or_child_path(path: &Path, root: &Path) -> bool {
    let path = path
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    let root = root
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase();
    path == root || path.starts_with(&format!("{root}\\"))
}

#[cfg(not(windows))]
fn is_same_or_child_path(path: &Path, root: &Path) -> bool {
    path == root || path.starts_with(root)
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
