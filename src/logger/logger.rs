pub fn setup_logger() {
    dotenvy::dotenv().ok();
    env_logger::init();
}
