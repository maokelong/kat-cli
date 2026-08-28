use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub fn cargo_kat() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_kat"))
}

#[allow(dead_code)]
pub fn response(output: std::process::Output) -> serde_json::Value {
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

#[allow(dead_code)]
pub fn repository_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(relative)
        .canonicalize()
        .unwrap()
}

#[allow(dead_code)]
pub fn assert_cpython_314(python: &Path) {
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
        "real-host E2E requires CPython 3.14"
    );
}

pub fn stage_skill(root: &Path, directory_name: &str) -> (PathBuf, PathBuf) {
    let skill = root.join(directory_name);
    let payload = skill
        .join("scripts")
        .join("targets")
        .join(platform_target());
    fs::create_dir_all(&payload).expect("create Platform Payload");
    fs::write(skill.join("SKILL.md"), "# KAT\n").expect("write Skill marker");
    let binary = payload.join(platform_binary());
    fs::copy(cargo_kat(), &binary).expect("copy kat into Skill");
    (skill, binary)
}

pub fn host_path(binary: &Path) -> PathBuf {
    let payload = binary.parent().expect("Platform Payload directory");
    if cfg!(windows) {
        payload.join("python").join("python.exe")
    } else {
        payload.join("python").join("bin").join("python3")
    }
}

pub fn stage_real_host_skill(
    root: &Path,
    kat_binary: &Path,
    python: &Path,
    workflow_wheel: &Path,
) -> (PathBuf, PathBuf) {
    let skill = root.join("staged-skill");
    let platform_payload = skill
        .join("scripts")
        .join("targets")
        .join(platform_target());
    fs::create_dir_all(&platform_payload).expect("create staged Platform Payload");
    fs::write(skill.join("SKILL.md"), "# KAT\n").expect("write Skill marker");
    prepare_real_host_payload(&platform_payload, python, workflow_wheel);
    fs::copy(kat_binary, platform_payload.join(platform_binary()))
        .expect("stage kat beside the real Workflow Host");
    let binary = skill
        .join("scripts")
        .join("targets")
        .join(platform_target())
        .join(platform_binary());
    (skill, binary)
}

fn platform_target() -> &'static str {
    if cfg!(windows) {
        "windows-x86_64"
    } else {
        "linux-x86_64"
    }
}

fn platform_binary() -> &'static str {
    if cfg!(windows) { "kat.exe" } else { "kat" }
}

fn prepare_real_host_payload(payload: &Path, python: &Path, workflow_wheel: &Path) {
    let environment = if cfg!(windows) {
        payload.to_path_buf()
    } else {
        payload.join("python")
    };
    let output = Command::new(python)
        .args(["-m", "venv"])
        .arg(&environment)
        .output()
        .expect("create real Workflow Host environment");
    assert!(
        output.status.success(),
        "Workflow Host environment creation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let environment_python = if cfg!(windows) {
        payload.join("Scripts").join("python.exe")
    } else {
        payload.join("python").join("bin").join("python3")
    };
    let output = Command::new(&environment_python)
        .args([
            "-m",
            "pip",
            "install",
            "--disable-pip-version-check",
            "--ignore-requires-python",
            "--no-index",
            "--find-links",
        ])
        .arg(
            workflow_wheel
                .parent()
                .expect("Workflow wheel belongs to a wheelhouse"),
        )
        .arg(workflow_wheel)
        .output()
        .expect("install Workflow Host wheel");
    assert!(
        output.status.success(),
        "Workflow Host wheel installation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    #[cfg(windows)]
    {
        let host = payload.join("python").join("python.exe");
        fs::create_dir_all(host.parent().unwrap()).expect("create real Host directory");
        fs::copy(environment_python, host).expect("stage real Windows Host executable");
    }
}
