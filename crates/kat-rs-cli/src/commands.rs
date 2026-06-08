use std::{io::Write, path::PathBuf, str::FromStr};

pub async fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match run_inner(args, out).await {
        Ok(()) => 0,
        Err(CommandError::Usage(message)) => {
            let _ = writeln!(err, "{message}");
            2
        }
        Err(CommandError::Runtime(error)) => {
            log::error!("command failed: {error:#}");
            let _ = writeln!(err, "{error:#}");
            1
        }
    }
}

async fn run_inner(args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
    match args.first().map(String::as_str) {
        None | Some("-h") | Some("--help") | Some("help") => {
            write_help(out).map_err(CommandError::from_runtime)?;
            Ok(())
        }
        Some("query") => run_query(&args[1..], out).await,
        Some(command) => Err(CommandError::Usage(format!("unknown command: {command}"))),
    }
}

async fn run_query(args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
    let args = QueryArgs::parse(args).map_err(CommandError::Usage)?;
    let mut session = kat_rs_session::Session::create();

    session
        .build_datasource(kat_rs_datasource::DataSourceConfig::new(
            args.source_type,
            args.file,
        ))
        .map_err(CommandError::from_runtime)?;

    let rows = session
        .query_json(&args.sql)
        .await
        .map_err(CommandError::from_runtime)?;

    serde_json::to_writer(&mut *out, &rows).map_err(CommandError::from_runtime)?;
    writeln!(out).map_err(CommandError::from_runtime)?;
    Ok(())
}

fn write_help(out: &mut dyn Write) -> std::io::Result<()> {
    writeln!(
        out,
        "usage: kat-rs query --source hitrace --file <path> --sql <sql>"
    )
}

struct QueryArgs {
    source_type: kat_rs_datasource::DataSourceType,
    file: PathBuf,
    sql: String,
}

impl QueryArgs {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut source_type = None;
        let mut file = None;
        let mut sql = None;
        let mut index = 0;

        while index < args.len() {
            match args[index].as_str() {
                "--source" => {
                    let value = read_arg_value(args, index, "--source")?;
                    source_type = Some(
                        kat_rs_datasource::DataSourceType::from_str(value)
                            .map_err(|error| error.to_string())?,
                    );
                    index += 2;
                }
                "--file" => {
                    file = Some(PathBuf::from(read_arg_value(args, index, "--file")?));
                    index += 2;
                }
                "--sql" => {
                    sql = Some(read_arg_value(args, index, "--sql")?.to_owned());
                    index += 2;
                }
                other => return Err(format!("unknown query argument: {other}")),
            }
        }

        Ok(Self {
            source_type: source_type.ok_or("missing required argument: --source")?,
            file: file.ok_or("missing required argument: --file")?,
            sql: sql.ok_or("missing required argument: --sql")?,
        })
    }
}

fn read_arg_value<'a>(args: &'a [String], index: usize, name: &str) -> Result<&'a str, String> {
    args.get(index + 1)
        .filter(|value| !value.starts_with("--"))
        .map(String::as_str)
        .ok_or_else(|| format!("missing value for argument: {name}"))
}

enum CommandError {
    Usage(String),
    Runtime(anyhow::Error),
}

impl CommandError {
    fn from_runtime(error: impl Into<anyhow::Error>) -> Self {
        Self::Runtime(error.into())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use kat_rs_datasource::proto::{HitraceEvent, HitraceTrace};
    use prost::Message;
    use tempfile::tempdir;

    use super::run;

    #[tokio::test]
    async fn query_command_prints_json_rows() {
        let dir = tempdir().expect("tempdir is created");
        let trace_path = dir.path().join("sample.hitrace");
        fs::write(&trace_path, encoded_trace()).expect("trace is written");

        let args = vec![
            "query".to_string(),
            "--source".to_string(),
            "hitrace".to_string(),
            "--file".to_string(),
            trace_path.to_string_lossy().to_string(),
            "--sql".to_string(),
            "select count(*) as count from hitrace_event".to_string(),
        ];
        let mut out = Vec::new();
        let mut err = Vec::new();

        let code = run(&args, &mut out, &mut err).await;

        assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&err));
        assert_eq!(String::from_utf8(out).expect("utf8"), "[{\"count\":2}]\n");
        assert!(err.is_empty());
    }

    #[tokio::test]
    async fn query_command_rejects_missing_required_arguments() {
        let args = vec![
            "query".to_string(),
            "--source".to_string(),
            "hitrace".to_string(),
        ];
        let mut out = Vec::new();
        let mut err = Vec::new();

        let code = run(&args, &mut out, &mut err).await;

        assert_eq!(code, 2);
        assert!(out.is_empty());
        assert!(
            String::from_utf8(err)
                .expect("utf8")
                .contains("missing required argument")
        );
    }

    #[tokio::test]
    async fn help_command_prints_usage() {
        let args = vec!["--help".to_string()];
        let mut out = Vec::new();
        let mut err = Vec::new();

        let code = run(&args, &mut out, &mut err).await;

        assert_eq!(code, 0);
        assert!(
            String::from_utf8(out)
                .expect("utf8")
                .contains("kat-rs query --source hitrace")
        );
        assert!(err.is_empty());
    }

    #[tokio::test]
    async fn query_command_rejects_unknown_source() {
        let args = vec![
            "query".to_string(),
            "--source".to_string(),
            "unknown".to_string(),
            "--file".to_string(),
            "sample.hitrace".to_string(),
            "--sql".to_string(),
            "select 1".to_string(),
        ];
        let mut out = Vec::new();
        let mut err = Vec::new();

        let code = run(&args, &mut out, &mut err).await;

        assert_eq!(code, 2);
        assert!(out.is_empty());
        assert!(
            String::from_utf8(err)
                .expect("utf8")
                .contains("unsupported datasource type")
        );
    }

    fn encoded_trace() -> Vec<u8> {
        HitraceTrace {
            events: vec![
                HitraceEvent {
                    timestamp_ns: 100,
                    pid: 10,
                    tid: 11,
                    tag: "sched".to_string(),
                    message: "wake up".to_string(),
                },
                HitraceEvent {
                    timestamp_ns: 200,
                    pid: 20,
                    tid: 21,
                    tag: "app".to_string(),
                    message: "start".to_string(),
                },
            ],
        }
        .encode_to_vec()
    }
}
