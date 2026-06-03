use std::io::Write;

pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match args.first().map(String::as_str) {
        None | Some("-h") | Some("--help") => {
            print_help(out);
            0
        }
        Some("datasource") => {
            let capability_count = kat_rs_datasource::capabilities().len();
            let _ = crate::output::write_line(
                out,
                &format!("datasource boundary ready with {capability_count} capability"),
            );
            0
        }
        Some(command) => {
            let _ = writeln!(err, "unknown command: {command}");
            2
        }
    }
}

fn print_help(out: &mut dyn Write) {
    let _ = crate::output::write_line(out, "kat-rs");
    let _ = crate::output::write_line(out, "");
    let _ = crate::output::write_line(out, "Usage:");
    let _ = crate::output::write_line(out, "  kat-rs <command>");
    let _ = crate::output::write_line(out, "");
    let _ = crate::output::write_line(out, "Commands:");
    let _ = crate::output::write_line(out, "  datasource    Trace datasource commands");
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn prints_help_without_args() {
        let mut out = Vec::new();
        let mut err = Vec::new();

        let code = run(&[], &mut out, &mut err);

        assert_eq!(code, 0);
        assert!(err.is_empty());
        assert!(String::from_utf8(out).unwrap().contains("kat-rs"));
    }

    #[test]
    fn rejects_unknown_command() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let args = vec!["unknown".to_string()];

        let code = run(&args, &mut out, &mut err);

        assert_eq!(code, 2);
        assert!(out.is_empty());
        assert!(String::from_utf8(err).unwrap().contains("unknown command"));
    }
}
