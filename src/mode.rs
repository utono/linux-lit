pub fn is_dev_mode() -> bool {
    std::env::var("LIT_DEV").is_ok()
}
