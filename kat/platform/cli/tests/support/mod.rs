use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub fn cargo_kat() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_kat"))
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
    stage_sdk_locator(&binary);
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
        .arg(std::env::var_os("KAT_TEST_SDK_WHEEL").expect("SDK wheel from current checkout"))
        .arg(
            std::env::var_os("KAT_TEST_DATASOURCE_WHEEL")
                .expect("Datasource wheel from current checkout"),
        )
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

fn stage_sdk_locator(binary: &Path) {
    use std::sync::OnceLock;
    static LOCATOR: OnceLock<Vec<u8>> = OnceLock::new();
    let bytes = LOCATOR.get_or_init(|| {
        let temporary = tempfile::tempdir().unwrap();
        let source = temporary.path().join("locator.rs");
        let executable = temporary.path().join("locator.exe");
        fs::write(&source, include_str!("sdk_locator.rs")).unwrap();
        let output = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
            .arg(source)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        fs::read(executable).unwrap()
    });
    let host = host_path(binary);
    fs::create_dir_all(host.parent().unwrap()).unwrap();
    fs::write(&host, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&host, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let sdk = host.parent().unwrap().join("sdk-fixture");
    fs::create_dir_all(&sdk).unwrap();
    fs::write(
        sdk.join("pack.toml"),
        r#"name = "kat-sdk"
title = "SDK"
description = "Official SDK fixture"
owner = "KAT tests"
"#,
    )
    .unwrap();
}
