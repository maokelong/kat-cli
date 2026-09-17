#![cfg(target_os = "linux")]

use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Read},
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[path = "support/process.rs"]
mod process;

fn shell(binary: &Path, record: &Path, exit_code: u8) -> Command {
    let mut command = Command::new(binary);
    command
        .args(["-c", "printf 'started\\n' >> \"$1\"; printf 'captured stdout'; printf 'captured stderr' >&2; if read -r line; then exit 99; fi; exit \"$2\"", "kat-process-test"])
        .arg(record)
        .arg(exit_code.to_string());
    command
}

#[test]
#[ignore = "由真实进程测试显式启动，用 stderr 同步首次重试"]
fn capture_probe() {
    let binary = std::env::var_os("KAT_TEST_CAPTURE_TARGET").unwrap();
    let record = std::env::var_os("KAT_TEST_CAPTURE_RECORD").unwrap();
    let result = process::output(&mut shell(Path::new(&binary), Path::new(&record), 0)).unwrap();
    assert!(result.status.success());
    assert_eq!(result.stdout, b"captured stdout");
    assert_eq!(result.stderr, b"captured stderr");
}

#[test]
fn temporary_writable_executable_is_retried_then_runs_once() {
    let temporary = tempfile::tempdir().unwrap();
    let binary = temporary.path().join("shell");
    let record = temporary.path().join("executions");
    fs::copy("/bin/sh", &binary).unwrap();
    let writer = OpenOptions::new().write(true).open(&binary).unwrap();
    let mut probe = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "capture_probe", "--ignored", "--nocapture"])
        .env("KAT_TEST_CAPTURE_TARGET", &binary)
        .env("KAT_TEST_CAPTURE_RECORD", &record)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stderr = probe.stderr.take().unwrap();
    let (ready, received) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut stderr = BufReader::new(stderr);
        let mut line = String::new();
        stderr.read_line(&mut line).unwrap();
        let _ = ready.send(line.clone());
        stderr.read_to_string(&mut line).unwrap();
        line
    });
    // 首次真实启动失败后才释放写句柄，不用睡眠猜测进程调度。
    let retry = received.recv_timeout(Duration::from_secs(5));
    drop(writer);
    if retry.is_err() {
        let _ = probe.kill();
    }
    let result = probe.wait_with_output().unwrap();
    let stderr = reader.join().unwrap();
    assert!(retry.unwrap().contains("[kat-test-spawn]"), "{stderr}");
    assert!(result.status.success(), "{result:?}\n{stderr}");
    assert_eq!(fs::read_to_string(record).unwrap(), "started\n");
}

#[test]
fn persistent_writable_executable_returns_original_error_within_budget() {
    let temporary = tempfile::tempdir().unwrap();
    let binary = temporary.path().join("shell");
    let record = temporary.path().join("executions");
    fs::copy("/bin/sh", &binary).unwrap();
    let _writer = OpenOptions::new().write(true).open(&binary).unwrap();
    let started = Instant::now();
    let error = process::output(&mut shell(&binary, &record, 0)).unwrap_err();

    assert_eq!(
        error.raw_os_error(),
        Some(rustix::io::Errno::TXTBSY.raw_os_error())
    );
    assert!(started.elapsed() >= Duration::from_secs(1));
    // 留出调度余量，但不能把启动重试变为无限等待。
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(!record.exists());
}

#[test]
fn missing_executable_fails_without_retry_delay() {
    let temporary = tempfile::tempdir().unwrap();
    let started = Instant::now();
    let error = process::output(&mut Command::new(temporary.path().join("missing"))).unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn non_executable_file_fails_without_retry_delay() {
    use std::os::unix::fs::PermissionsExt;

    let temporary = tempfile::tempdir().unwrap();
    let binary = temporary.path().join("shell");
    fs::copy("/bin/sh", &binary).unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o600)).unwrap();
    let started = Instant::now();
    let error = process::output(&mut Command::new(binary)).unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn successful_spawn_captures_output_and_never_retries_a_nonzero_exit() {
    let temporary = tempfile::tempdir().unwrap();
    for exit_code in [0, 7] {
        let record = temporary.path().join(format!("executions-{exit_code}"));
        let result = process::output(&mut shell(Path::new("/bin/sh"), &record, exit_code)).unwrap();

        assert_eq!(result.status.code(), Some(i32::from(exit_code)));
        assert_eq!(result.stdout, b"captured stdout");
        assert_eq!(result.stderr, b"captured stderr");
        assert_eq!(fs::read_to_string(record).unwrap(), "started\n");
    }
}
