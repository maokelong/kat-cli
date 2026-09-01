use std::process::Command;

#[allow(dead_code)]
mod support;
use support::cargo_kat;

#[test]
fn import_is_not_a_cli_operation() {
    let help = Command::new(cargo_kat()).arg("--help").output().unwrap();
    assert_eq!(help.status.code(), Some(0));
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(
        !help
            .lines()
            .any(|line| line.trim_start().starts_with("import ")),
        "removed import operation is still public: {help}"
    );

    let removed = Command::new(cargo_kat())
        .args(["import", "hitrace", "--trace", "capture.htrace"])
        .output()
        .unwrap();
    assert_eq!(removed.status.code(), Some(2));
    assert!(removed.stdout.is_empty());
    assert!(String::from_utf8_lossy(&removed.stderr).contains("unrecognized subcommand 'import'"));
}
