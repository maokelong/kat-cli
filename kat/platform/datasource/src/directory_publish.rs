use std::{fs, io, path::Path};

pub(crate) fn ensure_destination_absent(destination: &Path) -> io::Result<()> {
    match fs::symlink_metadata(destination) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "destination entry already exists: {}",
                destination.display()
            ),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(any(
    target_os = "android",
    target_os = "linux",
    target_os = "redox",
    target_vendor = "apple"
))]
pub(crate) fn publish_directory_no_replace(source: &Path, destination: &Path) -> io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};

    renameat_with(CWD, source, CWD, destination, RenameFlags::NOREPLACE).map_err(io::Error::from)
}

#[cfg(windows)]
pub(crate) fn publish_directory_no_replace(source: &Path, destination: &Path) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

    let source = windows_existing_path(source)?;
    let destination = windows_destination_path(destination)?;
    // Omitting MOVEFILE_REPLACE_EXISTING preserves the winner if another
    // process creates the destination after the initial preflight check.
    // SAFETY: both buffers are NUL-terminated and live for the duration of the
    // call; MoveFileExW does not retain either pointer.
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn windows_existing_path(path: &Path) -> io::Result<Vec<u16>> {
    null_terminated_wide_path(&path.canonicalize()?)
}

#[cfg(windows)]
fn windows_destination_path(path: &Path) -> io::Result<Vec<u16>> {
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "destination has no final path component",
        )
    })?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()?;
    // Only the parent is canonicalized: the final entry must not be followed.
    null_terminated_wide_path(&parent.join(name))
}

#[cfg(windows)]
fn null_terminated_wide_path(path: &Path) -> io::Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;

    let mut wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
    if wide.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path contains an interior NUL",
        ));
    }
    wide.push(0);
    Ok(wide)
}

#[cfg(not(any(
    target_os = "android",
    target_os = "linux",
    target_os = "redox",
    target_vendor = "apple",
    windows
)))]
pub(crate) fn publish_directory_no_replace(_source: &Path, _destination: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic no-replace directory publication is unsupported on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        path::Path,
        process::{Child, Command, Output, Stdio},
        thread,
        time::{Duration, Instant},
    };

    use super::{ensure_destination_absent, publish_directory_no_replace};

    const RACE_ROOT_ENV: &str = "KAT_DATASOURCE_DIRECTORY_PUBLISH_RACE_ROOT";
    const RACE_PUBLISHER_ENV: &str = "KAT_DATASOURCE_DIRECTORY_PUBLISH_RACE_PUBLISHER";

    #[test]
    fn existing_empty_directory_is_not_replaced() {
        let parent = tempfile::tempdir().expect("temporary parent is created");
        let staging = parent.path().join("staging");
        let destination = parent.path().join("destination");
        fs::create_dir(&staging).expect("staging directory is created");
        fs::create_dir(&destination).expect("destination directory is created");

        publish_directory_no_replace(&staging, &destination)
            .expect_err("an existing empty destination must not be replaced");

        assert!(staging.is_dir());
        assert!(destination.is_dir());
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn dangling_final_entry_is_detected_without_following_or_replacing_it() {
        let parent = tempfile::tempdir().expect("temporary parent is created");
        let staging = parent.path().join("staging");
        let missing_target = parent.path().join("missing-target");
        let destination = parent.path().join("destination");
        fs::create_dir(&staging).expect("staging directory is created");
        create_dangling_directory_link(&missing_target, &destination);

        let error = ensure_destination_absent(&destination)
            .expect_err("a dangling final entry is still an existing entry");
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        publish_directory_no_replace(&staging, &destination)
            .expect_err("a dangling final entry must not be replaced");

        assert!(staging.is_dir());
        assert!(is_link(
            &fs::symlink_metadata(&destination).expect("dangling link remains")
        ));
        assert!(!missing_target.exists());
    }

    #[cfg(any(
        target_os = "android",
        target_os = "linux",
        target_os = "redox",
        target_vendor = "apple",
        windows
    ))]
    #[test]
    fn simultaneous_process_publishers_have_exactly_one_winner() {
        let root = tempfile::tempdir().expect("race root is created");
        let executable = env::current_exe().expect("current test executable is available");
        let children = ["first", "second"].map(|publisher| {
            Command::new(&executable)
                .args([
                    "--exact",
                    "directory_publish::tests::publish_race_child",
                    "--nocapture",
                ])
                .env(RACE_ROOT_ENV, root.path())
                .env(RACE_PUBLISHER_ENV, publisher)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap_or_else(|error| panic!("failed to spawn {publisher} publisher: {error}"))
        });

        let ready = wait_until(Duration::from_secs(15), || {
            ["first", "second"]
                .into_iter()
                .all(|publisher| root.path().join(format!("ready-{publisher}")).is_file())
        });
        if !ready {
            let _ = fs::write(root.path().join("go"), []);
            let outputs = children.map(wait_for_child);
            panic!("publishers did not reach barrier: {outputs:#?}");
        }
        fs::write(root.path().join("go"), []).expect("race barrier is released");

        for output in children.map(wait_for_child) {
            assert!(
                output.status.success(),
                "publisher failed\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let mut outcomes = ["first", "second"].map(|publisher| {
            fs::read_to_string(root.path().join(format!("outcome-{publisher}")))
                .unwrap_or_else(|error| panic!("missing {publisher} outcome: {error}"))
        });
        outcomes.sort();
        assert_eq!(outcomes, ["lost", "won"]);
        assert!(root.path().join("destination").is_dir());
        assert_eq!(
            ["first", "second"]
                .into_iter()
                .filter(|publisher| root.path().join(format!("staging-{publisher}")).is_dir())
                .count(),
            1,
            "only the losing publisher keeps its staging directory"
        );
    }

    #[test]
    fn publish_race_child() {
        let Some(root) = env::var_os(RACE_ROOT_ENV).map(std::path::PathBuf::from) else {
            return;
        };
        let publisher = env::var(RACE_PUBLISHER_ENV).expect("publisher identity is set");
        let staging = root.join(format!("staging-{publisher}"));
        let destination = root.join("destination");
        fs::create_dir(&staging).expect("publisher staging is created");
        ensure_destination_absent(&destination)
            .expect("both publishers complete their preflight before the barrier");
        fs::write(root.join(format!("ready-{publisher}")), [])
            .expect("publisher signals readiness");
        assert!(
            wait_until(Duration::from_secs(15), || root.join("go").is_file()),
            "publisher timed out at barrier"
        );

        let outcome = match publish_directory_no_replace(&staging, &destination) {
            Ok(()) => {
                assert!(!staging.exists());
                "won"
            }
            Err(_) => {
                assert!(destination.is_dir(), "a winner must own the destination");
                assert!(staging.is_dir(), "loser must retain its staging directory");
                "lost"
            }
        };
        fs::write(root.join(format!("outcome-{publisher}")), outcome)
            .expect("publisher outcome is written");
    }

    fn wait_until(timeout: Duration, predicate: impl Fn() -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if predicate() {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        predicate()
    }

    fn wait_for_child(child: Child) -> Output {
        child
            .wait_with_output()
            .expect("publisher exits after barrier timeout")
    }

    #[cfg(unix)]
    fn create_dangling_directory_link(target: &Path, link: &Path) {
        std::os::unix::fs::symlink(target, link).expect("dangling directory symlink is created");
    }

    #[cfg(windows)]
    fn create_dangling_directory_link(target: &Path, link: &Path) {
        match std::os::windows::fs::symlink_dir(target, link) {
            Ok(()) => {}
            Err(error)
                if error.kind() == std::io::ErrorKind::PermissionDenied
                    || error.raw_os_error() == Some(1314) =>
            {
                fs::create_dir(target).expect("junction target is created");
                let output = Command::new("cmd")
                    .args(["/d", "/c", "mklink", "/j"])
                    .arg(link)
                    .arg(target)
                    .output()
                    .expect("junction command runs");
                assert!(
                    output.status.success(),
                    "failed to create junction\nstdout:\n{}\nstderr:\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                fs::remove_dir(target).expect("junction target is removed to make it dangling");
            }
            Err(error) => panic!("failed to create dangling directory symlink: {error}"),
        }
    }

    #[cfg(unix)]
    fn is_link(metadata: &fs::Metadata) -> bool {
        metadata.file_type().is_symlink()
    }

    #[cfg(windows)]
    fn is_link(metadata: &fs::Metadata) -> bool {
        use std::os::windows::fs::MetadataExt;

        metadata.file_attributes() & 0x400 != 0
    }
}
