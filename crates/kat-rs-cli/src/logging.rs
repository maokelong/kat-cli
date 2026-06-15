// 日志初始化保持可重复调用，便于二进制入口和测试共享。
pub fn init() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .try_init();
}
