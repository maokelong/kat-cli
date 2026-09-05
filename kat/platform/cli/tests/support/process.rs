use std::{
    io,
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

// 仅用于测试夹具的捕获式执行；不接受调用方自定义 stdio。
pub fn output(command: &mut Command) -> io::Result<Output> {
    const RETRY_BUDGET: Duration = Duration::from_secs(1);
    const RETRY_INTERVAL: Duration = Duration::from_millis(10);

    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let started = Instant::now();
    let mut attempts = 0;
    loop {
        attempts += 1;
        match command.spawn() {
            // 启动成功后不再重试，避免重复执行有副作用的测试命令。
            Ok(child) => return child.wait_with_output(),
            Err(error)
                if cfg!(target_os = "linux")
                    && error.kind() == io::ErrorKind::ExecutableFileBusy =>
            {
                let elapsed = started.elapsed();
                let Some(remaining) = RETRY_BUDGET.checked_sub(elapsed) else {
                    return Err(error);
                };
                if remaining.is_zero() {
                    return Err(error);
                }
                eprintln!(
                    "[kat-test-spawn] {:?}: attempt {attempts} failed after {elapsed:?}; retrying: {error}",
                    command.get_program(),
                );
                thread::sleep(RETRY_INTERVAL.min(remaining));
                if started.elapsed() >= RETRY_BUDGET {
                    return Err(error);
                }
            }
            Err(error) => return Err(error),
        }
    }
}
