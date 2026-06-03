pub fn init() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("off"))
        .try_init();
}
