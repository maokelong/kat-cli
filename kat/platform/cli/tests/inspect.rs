use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn cargo_kat() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_kat"))
}

fn stage_minimum_skill_layout(root: &Path) -> (PathBuf, PathBuf) {
    let skill = root.join("movable-skill");
    let target = if cfg!(windows) {
        "windows-x86_64"
    } else {
        "linux-x86_64"
    };
    let binary_name = if cfg!(windows) { "kat.exe" } else { "kat" };
    let payload = skill.join("scripts").join("targets").join(target);
    fs::create_dir_all(&payload).expect("create Platform Payload");
    fs::write(skill.join("SKILL.md"), "# KAT\n").expect("write Skill marker");
    let binary = payload.join(binary_name);
    fs::copy(cargo_kat(), &binary).expect("copy kat into Skill");
    (skill, binary)
}

fn prepare_platform_data_home(command: &mut Command, root: &Path) {
    #[cfg(not(windows))]
    command
        .env("XDG_DATA_HOME", root.join("xdg-data"))
        .env("HOME", root.join("home"));
    #[cfg(windows)]
    {
        // ProjectDirs 使用 Windows Known Folder API；进程测试因此使用干净 runner 的真实 Data Home。
        let _ = (command, root);
        assert_clean_windows_data_home();
    }
}

#[cfg(not(windows))]
fn data_home(root: &Path) -> PathBuf {
    root.join("xdg-data").join("kat")
}

#[cfg(windows)]
fn assert_clean_windows_data_home() {
    let project_dirs = directories::ProjectDirs::from("", "", "KAT")
        .expect("Windows runner has a standard user data directory");
    let pack_directory = project_dirs.data_dir().join("packs");
    assert!(
        !pack_directory.exists(),
        "Windows real-process tests require a clean runner without {pack_directory:?}"
    );
}

fn write_pack(directory: &Path, name: &str, description: &str) -> String {
    fs::create_dir_all(directory).expect("create PACK directory");
    let manifest = format!(
        "name = {name:?}\ntitle = {name:?}\ndescription = {description:?}\nowner = \"Test Team\"\n"
    );
    fs::write(directory.join("pack.toml"), &manifest).expect("write PACK manifest");
    manifest
}

#[test]
fn help_and_parse_failures_do_not_require_a_skill_layout() {
    let help = Command::new(cargo_kat())
        .arg("--help")
        .output()
        .expect("run help");
    assert_eq!(help.status.code(), Some(0));
    assert!(help.stderr.is_empty());
    let help_text = String::from_utf8(help.stdout).expect("UTF-8 help");
    assert!(help_text.contains("inspect"));
    assert!(!help_text.starts_with('{'));

    let operation_help = Command::new(cargo_kat())
        .args(["inspect", "--help"])
        .output()
        .expect("run operation help");
    assert_eq!(operation_help.status.code(), Some(0));
    assert!(operation_help.stderr.is_empty());
    assert!(!operation_help.stdout.starts_with(b"{"));
    let operation_help_text = String::from_utf8(operation_help.stdout).expect("UTF-8 help");
    assert!(operation_help_text.contains("validation order"));
    assert!(operation_help_text.contains("sorted by PACK name"));

    for arguments in [
        Vec::<&str>::new(),
        vec!["unknown"],
        vec!["inspect", "--version"],
    ] {
        let output = Command::new(cargo_kat())
            .args(arguments)
            .output()
            .expect("run parse failure");
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn inspect_lists_all_packs_from_a_moved_skill_and_arbitrary_cwd() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (skill, binary) = stage_minimum_skill_layout(temporary.path());
    let bundled = skill.join("assets").join("packs").join("bundled-directory");
    let bundled_manifest = write_pack(&bundled, "bravo", "Bundled description");
    let cwd = temporary.path().join("unrelated-cwd");
    let additional = cwd.join("relative-pack");
    let additional_manifest = write_pack(&additional, "alpha", "External description");
    #[cfg(not(windows))]
    let (data_pack, data_manifest) = {
        let data_pack = data_home(temporary.path())
            .join("packs")
            .join("data-directory");
        let data_manifest = write_pack(&data_pack, "charlie", "Data Home description");
        (data_pack, data_manifest)
    };
    let moved_skill = temporary.path().join("moved-again");
    fs::rename(&skill, &moved_skill).expect("move minimum Skill layout");
    let moved_binary = moved_skill.join(binary.strip_prefix(&skill).unwrap());
    let moved_bundled = moved_skill.join(bundled.strip_prefix(&skill).unwrap());

    let mut command = Command::new(moved_binary);
    command
        .current_dir(&cwd)
        .args(["inspect", "--pack-dir", "relative-pack"]);
    prepare_platform_data_home(&mut command, temporary.path());
    let output = command.output().expect("run staged kat inspect");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    #[cfg(not(windows))]
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "{\"status\":\"success\",\"result\":{\"packs\":[{\"name\":\"alpha\",\"title\":\"alpha\",\"description\":\"External description\",\"owner\":\"Test Team\"},{\"name\":\"bravo\",\"title\":\"bravo\",\"description\":\"Bundled description\",\"owner\":\"Test Team\"},{\"name\":\"charlie\",\"title\":\"charlie\",\"description\":\"Data Home description\",\"owner\":\"Test Team\"}]}}\n"
    );
    #[cfg(windows)]
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "{\"status\":\"success\",\"result\":{\"packs\":[{\"name\":\"alpha\",\"title\":\"alpha\",\"description\":\"External description\",\"owner\":\"Test Team\"},{\"name\":\"bravo\",\"title\":\"bravo\",\"description\":\"Bundled description\",\"owner\":\"Test Team\"}]}}\n"
    );
    assert_eq!(
        fs::read_to_string(moved_bundled.join("pack.toml")).unwrap(),
        bundled_manifest
    );
    assert_eq!(
        fs::read_to_string(additional.join("pack.toml")).unwrap(),
        additional_manifest
    );
    #[cfg(not(windows))]
    assert_eq!(
        fs::read_to_string(data_pack.join("pack.toml")).unwrap(),
        data_manifest
    );
    assert_eq!(
        fs::read_to_string(moved_skill.join("SKILL.md")).unwrap(),
        "# KAT\n"
    );
    assert_eq!(fs::read_dir(&moved_bundled).unwrap().count(), 1);
    assert_eq!(fs::read_dir(&additional).unwrap().count(), 1);
    #[cfg(not(windows))]
    assert!(!data_home(temporary.path()).join("logs").exists());
}

#[test]
fn formed_operation_rejects_a_binary_outside_the_minimum_skill_layout() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let mut command = Command::new(cargo_kat());
    command.arg("inspect");
    prepare_platform_data_home(&mut command, temporary.path());

    let output = command.output().expect("run unstaged kat inspect");

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "failure");
    assert_eq!(response["error"]["message"], "KAT Skill is unavailable");
    assert!(response.get("result").is_none());
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.contains("KAT Skill is unavailable"));
    assert!(diagnostic.contains("<skill>/scripts/targets/<target>"));
    assert!(diagnostic.contains("regular <skill>/"));
    assert!(diagnostic.contains("SKILL.md marker"));
    #[cfg(not(windows))]
    assert!(!data_home(temporary.path()).exists());
}

#[test]
fn formed_operation_failure_is_one_json_response_and_readable_diagnostic() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (_skill, binary) = stage_minimum_skill_layout(temporary.path());
    let broken = temporary.path().join("broken-pack");
    fs::create_dir_all(&broken).unwrap();
    fs::write(broken.join("pack.toml"), "not valid TOML = [").unwrap();
    let mut command = Command::new(binary);
    command.arg("inspect").arg("--pack-dir").arg(&broken);
    prepare_platform_data_home(&mut command, temporary.path());

    let output = command.output().expect("run failed inspection");

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).expect("failure JSON");
    assert_eq!(response["status"], "failure");
    assert!(response.get("result").is_none());
    assert!(response.get("log_path").is_none());
    assert_eq!(response["error"]["message"], "PACK discovery failed");
    assert!(response["error"]["causes"].as_array().unwrap().len() >= 2);
    assert!(String::from_utf8_lossy(&output.stderr).contains("PACK discovery failed"));
    #[cfg(not(windows))]
    assert!(!data_home(temporary.path()).join("logs").exists());
}

#[test]
fn absent_default_directories_are_an_empty_result_and_are_not_created() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (skill, binary) = stage_minimum_skill_layout(temporary.path());
    let mut command = Command::new(binary);
    command.arg("inspect");
    prepare_platform_data_home(&mut command, temporary.path());

    let output = command.output().expect("run empty inspection");

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "{\"status\":\"success\",\"result\":{\"packs\":[]}}\n"
    );
    assert!(!skill.join("assets").join("packs").exists());
    #[cfg(not(windows))]
    assert!(!data_home(temporary.path()).exists());
}

#[test]
fn closed_stdout_makes_the_real_process_fail() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (skill, binary) = stage_minimum_skill_layout(temporary.path());
    write_pack(
        &skill.join("assets").join("packs").join("large"),
        "large",
        &"x".repeat(2 * 1024 * 1024),
    );
    let mut command = Command::new(binary);
    command
        .arg("inspect")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    prepare_platform_data_home(&mut command, temporary.path());
    let mut child = command.spawn().expect("spawn kat inspect");
    drop(child.stdout.take());

    let output = child.wait_with_output().expect("wait for kat inspect");

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("write KAT Response"));
}
