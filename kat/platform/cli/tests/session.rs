use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[path = "support/parquet.rs"]
mod parquet_fixture;

const SESSION_ID: &str = "019f6e00-0000-7000-8000-000000000040";
const RUN_A: &str = "019f6e00-0000-7000-8000-000000000041";
const RUN_B: &str = "019f6e00-0000-7000-8000-000000000042";
const CANDIDATE: &str = "019f6e00-0000-7000-8000-000000000043";

fn cargo_kat() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_kat"))
}

fn data_home(root: &Path) -> PathBuf {
    root.join("data-home")
}

fn command_with_data_home(home: &Path) -> Command {
    fs::create_dir_all(home).unwrap();
    let mut command = Command::new(cargo_kat());
    command.env("KAT_DATA_HOME", home);
    command
}

fn command(root: &Path) -> Command {
    command_with_data_home(&data_home(root))
}

#[cfg(windows)]
#[allow(clippy::permissions_set_readonly_false)]
fn make_writable(path: &Path) {
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions).unwrap();
}

#[cfg(unix)]
fn make_writable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(permissions.mode() | 0o200);
    fs::set_permissions(path, permissions).unwrap();
}

fn write_published_session_in(home: &Path) -> PathBuf {
    let sessions = home.join("sessions");
    let session = sessions.join(SESSION_ID);
    fs::create_dir_all(sessions.join(".leases")).unwrap();
    fs::create_dir_all(sessions.join(".deletions")).unwrap();
    fs::write(
        sessions.join(".leases").join(format!("{SESSION_ID}.lock")),
        [],
    )
    .unwrap();
    fs::create_dir_all(session.join("materializations")).unwrap();
    fs::create_dir_all(session.join("scratch")).unwrap();
    fs::create_dir_all(session.join("runs")).unwrap();
    fs::write(
        session.join("session.json"),
        serde_json::to_vec(&serde_json::json!({"session_id": SESSION_ID})).unwrap(),
    )
    .unwrap();
    let mut permissions = fs::metadata(session.join("session.json"))
        .unwrap()
        .permissions();
    permissions.set_readonly(true);
    fs::set_permissions(session.join("session.json"), permissions).unwrap();
    write_run(&session, RUN_B, "pack-b", "workflow-b", "summary", 2);
    write_run(&session, RUN_A, "pack-a", "workflow-a", "main", 1);
    fs::create_dir(session.join("runs").join(CANDIDATE)).unwrap();
    session
}

fn write_published_session(root: &Path) -> PathBuf {
    write_published_session_in(&data_home(root))
}

fn write_run(
    session: &Path,
    run_id: &str,
    pack: &str,
    workflow: &str,
    output: &str,
    row_count: u64,
) {
    let run = session.join("runs").join(run_id);
    fs::create_dir_all(run.join("outputs")).unwrap();
    let values = (0..row_count).map(|value| value as i64).collect::<Vec<_>>();
    parquet_fixture::write_i64(
        &run.join("outputs").join(format!("{output}.parquet")),
        "value",
        &values,
    );
    fs::write(
        run.join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "session_id": SESSION_ID,
            "run_id": run_id,
            "pack": pack,
            "workflow": workflow,
            "child_runs": if run_id == RUN_A { vec![RUN_B] } else { vec![] },
            "inputs": {},
            "outputs": {
                output: {
                    "columns": [{"name": "value", "type": "int64"}],
                    "row_count": row_count
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
}

fn write_legacy_layout_sentinels(home: &Path) -> (PathBuf, PathBuf) {
    let run = home.join("runs").join(RUN_A).join("manifest.json");
    let datasource = home
        .join("datasources")
        .join("legacy-pack")
        .join("sentinel");
    fs::create_dir_all(run.parent().unwrap()).unwrap();
    fs::create_dir_all(datasource.parent().unwrap()).unwrap();
    fs::write(&run, b"legacy root must not be read").unwrap();
    fs::write(&datasource, b"legacy datasource").unwrap();
    (run, datasource)
}

fn assert_legacy_layout_sentinels(run: &Path, datasource: &Path) {
    assert_eq!(fs::read(run).unwrap(), b"legacy root must not be read");
    assert_eq!(fs::read(datasource).unwrap(), b"legacy datasource");
}

#[test]
fn session_create_publishes_an_empty_session_with_one_generated_identity() {
    let temporary = tempfile::tempdir().unwrap();

    let output = command(temporary.path())
        .args(["session", "create"])
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let session_id = response["result"]["session_id"].as_str().unwrap();
    assert_eq!(
        response,
        serde_json::json!({
            "status": "success",
            "result": {"session_id": session_id}
        })
    );
    let identity = uuid::Uuid::parse_str(session_id).unwrap();
    assert_eq!(identity.get_version_num(), 7);

    let home = data_home(temporary.path());
    let session = home.join("sessions").join(session_id);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(
            &fs::read(session.join("session.json")).unwrap()
        )
        .unwrap(),
        serde_json::json!({"session_id": session_id})
    );
    assert!(session.join("materializations").is_dir());
    assert!(session.join("scratch").is_dir());
    assert!(session.join("runs").is_dir());
    assert_eq!(fs::read_dir(session.join("runs")).unwrap().count(), 0);
    assert!(
        home.join("sessions/.leases")
            .join(format!("{session_id}.lock"))
            .is_file()
    );
}

#[test]
fn session_create_accepts_no_caller_supplied_identity() {
    let temporary = tempfile::tempdir().unwrap();

    let output = command(temporary.path())
        .args(["session", "create", "--session", SESSION_ID])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected argument '--session'"));
    assert!(!data_home(temporary.path()).join("sessions").exists());
}

#[test]
fn inspect_empty_created_session_returns_an_empty_run_inventory() {
    let temporary = tempfile::tempdir().unwrap();
    let created = command(temporary.path())
        .args(["session", "create"])
        .output()
        .unwrap();
    assert_eq!(created.status.code(), Some(0));
    let created: serde_json::Value = serde_json::from_slice(&created.stdout).unwrap();
    let session_id = created["result"]["session_id"].as_str().unwrap();

    let output = command(temporary.path())
        .args(["inspect", "session", "--session", session_id])
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::json!({
            "status": "success",
            "result": {
                "session_id": session_id,
                "runs": []
            }
        })
    );
}

#[test]
fn inspect_session_returns_sorted_published_run_inventory() {
    let temporary = tempfile::tempdir().unwrap();
    write_published_session(temporary.path());
    let (legacy_run, legacy_datasource) =
        write_legacy_layout_sentinels(&data_home(temporary.path()));

    let output = command(temporary.path())
        .args(["inspect", "session", "--session", SESSION_ID])
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        response,
        serde_json::json!({
            "status": "success",
            "result": {
                "session_id": SESSION_ID,
                "runs": [
                    {
                        "run_id": RUN_A,
                        "pack": "pack-a",
                        "workflow": "workflow-a",
                        "child_runs": [RUN_B],
                        "outputs": {
                            "main": {
                                "columns": [{"name": "value", "type": "int64"}],
                                "row_count": 1
                            }
                        }
                    },
                    {
                        "run_id": RUN_B,
                        "pack": "pack-b",
                        "workflow": "workflow-b",
                        "child_runs": [],
                        "outputs": {
                            "summary": {
                                "columns": [{"name": "value", "type": "int64"}],
                                "row_count": 2
                            }
                        }
                    }
                ]
            }
        })
    );
    assert_legacy_layout_sentinels(&legacy_run, &legacy_datasource);
}

#[test]
fn active_session_inspection_blocks_delete_while_its_response_is_being_written() {
    let temporary = tempfile::tempdir().unwrap();
    let session = write_published_session(temporary.path());
    let manifest = session.join("runs").join(RUN_A).join("manifest.json");
    let mut document: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    document["workflow"] = serde_json::Value::String("w".repeat(8 * 1024 * 1024));
    fs::write(manifest, serde_json::to_vec(&document).unwrap()).unwrap();

    let mut inspecting = command(temporary.path());
    inspecting
        .args(["inspect", "session", "--session", SESSION_ID])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut inspecting = inspecting.spawn().unwrap();
    let mut stdout = inspecting.stdout.take().unwrap();
    let mut first_byte = [0];
    stdout.read_exact(&mut first_byte).unwrap();
    assert_eq!(first_byte, [b'{']);

    let mut deleting = command(temporary.path());
    deleting
        .args(["session", "delete", "--session", SESSION_ID])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut deleting = deleting.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let delete_completed = loop {
        if deleting.try_wait().unwrap().is_some() {
            break true;
        }
        if Instant::now() >= deadline {
            deleting.kill().unwrap();
            break false;
        }
        thread::sleep(Duration::from_millis(10));
    };
    let blocked = deleting.wait_with_output().unwrap();
    let session_remained_while_blocked = session.is_dir();
    let tombstone_remained_absent = !data_home(temporary.path())
        .join("sessions/.deletions")
        .join(SESSION_ID)
        .exists();

    let mut frame = Vec::with_capacity(8 * 1024 * 1024);
    frame.extend_from_slice(&first_byte);
    stdout.read_to_end(&mut frame).unwrap();
    let status = inspecting.wait().unwrap();
    assert_eq!(status.code(), Some(0));
    let response: serde_json::Value = serde_json::from_slice(&frame).unwrap();
    assert_eq!(response["result"]["session_id"], SESSION_ID);

    let deleted = command(temporary.path())
        .args(["session", "delete", "--session", SESSION_ID])
        .output()
        .unwrap();
    assert!(
        delete_completed,
        "Session delete did not fail immediately while inspection was active"
    );
    assert_eq!(blocked.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&blocked.stdout).unwrap();
    assert_eq!(
        response["error"]["message"],
        format!("Analysis Session {SESSION_ID} is in use")
    );
    assert!(session_remained_while_blocked);
    assert!(tombstone_remained_absent);
    assert_eq!(
        deleted.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&deleted.stderr)
    );
    assert!(!session.exists());
    assert!(
        data_home(temporary.path())
            .join("sessions/.leases")
            .join(format!("{SESSION_ID}.lock"))
            .is_file()
    );
    assert!(
        !data_home(temporary.path())
            .join("sessions/.deletions")
            .join(SESSION_ID)
            .exists()
    );
}

#[test]
fn inspect_session_fails_atomically_for_one_corrupt_published_run() {
    let temporary = tempfile::tempdir().unwrap();
    let session = write_published_session(temporary.path());
    let manifest = session.join("runs").join(RUN_B).join("manifest.json");
    let mut document: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    document["session_id"] = serde_json::json!("019f6e00-0000-7000-8000-000000000099");
    fs::write(manifest, serde_json::to_vec(&document).unwrap()).unwrap();

    let output = command(temporary.path())
        .args(["inspect", "session", "--session", SESSION_ID])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["error"]["message"], "Run is corrupted");
    assert!(response.get("result").is_none());
}

#[test]
fn session_delete_removes_only_the_session_and_retains_its_lease_identity() {
    let temporary = tempfile::tempdir().unwrap();
    let session = write_published_session(temporary.path());
    let lease = data_home(temporary.path())
        .join("sessions/.leases")
        .join(format!("{SESSION_ID}.lock"));
    let log = data_home(temporary.path()).join("logs/retained.log");
    let query = data_home(temporary.path()).join("query-results/retained.ndjson");
    fs::create_dir_all(log.parent().unwrap()).unwrap();
    fs::create_dir_all(query.parent().unwrap()).unwrap();
    fs::write(&log, b"log").unwrap();
    fs::write(&query, b"query").unwrap();
    let (legacy_run, legacy_datasource) =
        write_legacy_layout_sentinels(&data_home(temporary.path()));

    let output = command(temporary.path())
        .args(["session", "delete", "--session", SESSION_ID])
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::json!({
            "status": "success",
            "result": {"session_id": SESSION_ID}
        })
    );
    assert!(!session.exists());
    assert!(lease.is_file());
    assert!(
        !data_home(temporary.path())
            .join("sessions/.deletions")
            .join(SESSION_ID)
            .exists()
    );
    assert_eq!(fs::read(log).unwrap(), b"log");
    assert_eq!(fs::read(query).unwrap(), b"query");
    assert_legacy_layout_sentinels(&legacy_run, &legacy_datasource);
}

#[test]
fn session_delete_retries_one_fixed_partial_tombstone() {
    let temporary = tempfile::tempdir().unwrap();
    let session = write_published_session(temporary.path());
    let tombstone = data_home(temporary.path())
        .join("sessions")
        .join(".deletions")
        .join(SESSION_ID);
    fs::rename(&session, &tombstone).unwrap();
    fs::remove_dir_all(&tombstone).unwrap();
    fs::create_dir(&tombstone).unwrap();
    fs::write(tombstone.join("partial"), b"interrupted deletion").unwrap();

    let output = command(temporary.path())
        .args(["session", "delete", "--session", SESSION_ID])
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!tombstone.exists());
    assert!(
        data_home(temporary.path())
            .join("sessions/.leases")
            .join(format!("{SESSION_ID}.lock"))
            .is_file()
    );
}

#[test]
fn session_delete_fails_closed_when_source_and_tombstone_both_exist() {
    let temporary = tempfile::tempdir().unwrap();
    let session = write_published_session(temporary.path());
    let tombstone = data_home(temporary.path())
        .join("sessions/.deletions")
        .join(SESSION_ID);
    fs::create_dir(&tombstone).unwrap();
    fs::write(tombstone.join("sentinel"), b"do not replace").unwrap();

    let output = command(temporary.path())
        .args(["session", "delete", "--session", SESSION_ID])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        response["error"]["message"],
        "Analysis Session deletion state is corrupted"
    );
    assert!(session.is_dir());
    assert_eq!(
        fs::read(tombstone.join("sentinel")).unwrap(),
        b"do not replace"
    );
}

#[test]
fn session_delete_fails_closed_for_a_corrupt_public_marker() {
    let temporary = tempfile::tempdir().unwrap();
    let session = write_published_session(temporary.path());
    let marker = session.join("session.json");
    make_writable(&marker);
    fs::write(
        &marker,
        br#"{"session_id":"019f6e00-0000-7000-8000-000000000099"}"#,
    )
    .unwrap();

    let output = command(temporary.path())
        .args(["session", "delete", "--session", SESSION_ID])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(session.is_dir());
    assert!(
        !data_home(temporary.path())
            .join("sessions/.deletions")
            .join(SESSION_ID)
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn session_delete_never_follows_a_public_session_symlink() {
    use std::os::unix::fs::symlink;

    let temporary = tempfile::tempdir().unwrap();
    let session = write_published_session(temporary.path());
    let external = temporary.path().join("external-session");
    fs::rename(&session, &external).unwrap();
    let sentinel = external.join("sentinel");
    fs::write(&sentinel, b"outside Session storage").unwrap();
    symlink(&external, &session).unwrap();
    let tombstone = data_home(temporary.path())
        .join("sessions/.deletions")
        .join(SESSION_ID);

    let output = command(temporary.path())
        .args(["session", "delete", "--session", SESSION_ID])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        response["error"]["message"],
        "Analysis Session layout is invalid"
    );
    assert!(response.get("result").is_none());
    assert!(!tombstone.exists());
    assert_eq!(fs::read(&sentinel).unwrap(), b"outside Session storage");
    assert!(
        fs::symlink_metadata(&session)
            .unwrap()
            .file_type()
            .is_symlink()
    );

    fs::remove_file(session).unwrap();
    make_writable(&external.join("session.json"));
}

#[cfg(windows)]
#[test]
fn session_delete_never_follows_a_public_session_junction() {
    use std::os::windows::fs::MetadataExt;

    let temporary = tempfile::tempdir().unwrap();
    let session = write_published_session(temporary.path());
    let external = temporary.path().join("external-session");
    fs::rename(&session, &external).unwrap();
    let sentinel = external.join("sentinel");
    fs::write(&sentinel, b"outside Session storage").unwrap();
    let junction = Command::new("cmd.exe")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(&session)
        .arg(&external)
        .output()
        .unwrap();
    assert!(
        junction.status.success(),
        "{}",
        String::from_utf8_lossy(&junction.stderr)
    );
    let tombstone = data_home(temporary.path())
        .join("sessions/.deletions")
        .join(SESSION_ID);

    let output = command(temporary.path())
        .args(["session", "delete", "--session", SESSION_ID])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        response["error"]["message"],
        "Analysis Session layout is invalid"
    );
    assert!(response.get("result").is_none());
    assert!(!tombstone.exists());
    assert_eq!(fs::read(&sentinel).unwrap(), b"outside Session storage");
    assert_ne!(
        fs::symlink_metadata(&session).unwrap().file_attributes() & 0x400,
        0
    );

    fs::remove_dir(session).unwrap();
    make_writable(&external.join("session.json"));
}

#[cfg(unix)]
#[test]
fn session_delete_never_follows_a_tombstone_symlink() {
    use std::os::unix::fs::symlink;

    let temporary = tempfile::tempdir().unwrap();
    let session = write_published_session(temporary.path());
    fs::remove_dir_all(session).unwrap();
    let external = temporary.path().join("external");
    fs::create_dir(&external).unwrap();
    fs::write(external.join("sentinel"), b"outside Session storage").unwrap();
    let tombstone = data_home(temporary.path())
        .join("sessions/.deletions")
        .join(SESSION_ID);
    symlink(&external, &tombstone).unwrap();

    let output = command(temporary.path())
        .args(["session", "delete", "--session", SESSION_ID])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        response["error"]["message"],
        "Analysis Session deletion tombstone is invalid"
    );
    assert!(response.get("result").is_none());
    assert_eq!(
        fs::read(external.join("sentinel")).unwrap(),
        b"outside Session storage"
    );
    assert!(tombstone.is_symlink());
}

#[cfg(windows)]
#[test]
fn session_delete_never_follows_a_tombstone_junction() {
    let temporary = tempfile::tempdir().unwrap();
    let session = write_published_session(temporary.path());
    make_writable(&session.join("session.json"));
    fs::remove_dir_all(session).unwrap();
    let external = temporary.path().join("external");
    fs::create_dir(&external).unwrap();
    fs::write(external.join("sentinel"), b"outside Session storage").unwrap();
    let tombstone = data_home(temporary.path())
        .join("sessions")
        .join(".deletions")
        .join(SESSION_ID);
    let junction = Command::new("cmd.exe")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(&tombstone)
        .arg(&external)
        .output()
        .unwrap();
    assert!(
        junction.status.success(),
        "{}",
        String::from_utf8_lossy(&junction.stderr)
    );

    let output = command(temporary.path())
        .args(["session", "delete", "--session", SESSION_ID])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        fs::read(external.join("sentinel")).unwrap(),
        b"outside Session storage"
    );
    use std::os::windows::fs::MetadataExt;
    assert_ne!(
        fs::symlink_metadata(&tombstone).unwrap().file_attributes() & 0x400,
        0
    );
    fs::remove_dir(tombstone).unwrap();
}

#[cfg(windows)]
#[test]
fn session_delete_supports_a_long_explicit_data_home() {
    use std::os::windows::ffi::OsStrExt;

    let temporary = tempfile::tempdir().unwrap();
    let long_home = temporary
        .path()
        .join("a".repeat(120))
        .join("b".repeat(120))
        .join("c".repeat(80));
    assert!(long_home.as_os_str().encode_wide().count() > 300);
    let session = write_published_session_in(&long_home);

    let output = command_with_data_home(&long_home)
        .args(["session", "delete", "--session", SESSION_ID])
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::json!({
            "status": "success",
            "result": {"session_id": SESSION_ID}
        })
    );
    assert!(!session.exists());
    assert!(
        long_home
            .join("sessions")
            .join(".leases")
            .join(format!("{SESSION_ID}.lock"))
            .is_file()
    );
    assert!(
        !long_home
            .join("sessions")
            .join(".deletions")
            .join(SESSION_ID)
            .exists()
    );
}

#[test]
fn public_session_and_run_ids_reject_path_traversal() {
    let temporary = tempfile::tempdir().unwrap();
    write_published_session(temporary.path());
    let external = data_home(temporary.path()).join("external");
    fs::create_dir(&external).unwrap();
    let sentinel = external.join("sentinel");
    fs::write(&sentinel, b"outside identity roots").unwrap();

    for (arguments, message) in [
        (
            vec!["inspect", "session", "--session", "../external"],
            "Analysis Session ../external does not exist".to_owned(),
        ),
        (
            vec![
                "inspect",
                "workflow",
                "--session",
                SESSION_ID,
                "--run",
                "../../../external",
            ],
            format!("Run ../../../external does not exist in Analysis Session {SESSION_ID}"),
        ),
    ] {
        let output = command(temporary.path()).args(&arguments).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{arguments:?}");
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["status"], "failure", "{arguments:?}");
        assert_eq!(response["error"]["message"], message, "{arguments:?}");
        assert!(response.get("result").is_none(), "{arguments:?}");
        assert_eq!(
            fs::read(&sentinel).unwrap(),
            b"outside identity roots",
            "{arguments:?}"
        );
    }
}

#[test]
fn deleted_session_is_not_recovered_from_its_permanent_lease() {
    let temporary = tempfile::tempdir().unwrap();
    write_published_session(temporary.path());

    let first = command(temporary.path())
        .args(["session", "delete", "--session", SESSION_ID])
        .output()
        .unwrap();
    assert_eq!(first.status.code(), Some(0));

    let second = command(temporary.path())
        .args(["session", "delete", "--session", SESSION_ID])
        .output()
        .unwrap();
    assert_eq!(second.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(
        response["error"]["message"],
        format!("Analysis Session {SESSION_ID} does not exist")
    );
}

#[test]
fn two_delete_processes_can_never_both_delete_one_session() {
    let temporary = tempfile::tempdir().unwrap();
    write_published_session(temporary.path());

    let mut children = Vec::new();
    for _ in 0..2 {
        let mut deleting = command(temporary.path());
        deleting
            .args(["session", "delete", "--session", SESSION_ID])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        children.push(deleting.spawn().unwrap());
    }
    let outputs = children
        .into_iter()
        .map(|child| child.wait_with_output().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(
        outputs
            .iter()
            .filter(|output| output.status.success())
            .count(),
        1,
        "{outputs:?}"
    );
    let failure = outputs
        .iter()
        .find(|output| !output.status.success())
        .unwrap();
    let response: serde_json::Value = serde_json::from_slice(&failure.stdout).unwrap();
    let message = response["error"]["message"].as_str().unwrap();
    assert!(
        message.ends_with(" is in use") || message.ends_with(" does not exist"),
        "unexpected losing delete diagnostic: {message}"
    );
    assert!(
        !data_home(temporary.path())
            .join("sessions")
            .join(SESSION_ID)
            .exists()
    );
    assert!(
        !data_home(temporary.path())
            .join("sessions/.deletions")
            .join(SESSION_ID)
            .exists()
    );
}
