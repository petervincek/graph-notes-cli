pub fn setup_logger(maybe_log_level: &Option<String>) {
    dotenvy::dotenv().ok();
    let mut builder = env_logger::Builder::from_default_env();
    if let Some(log_level) = maybe_log_level {
        builder.parse_filters(log_level);
    }
    builder.init();
}
