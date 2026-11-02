#![allow(dead_code)]

pub fn url() -> Option<String> {
    if let Ok(url) = std::env::var("DATABASE_URL") {
        return Some(url);
    }
    assert!(
        std::env::var("REQUIRE_DATABASE_URL").is_err(),
        "REQUIRE_DATABASE_URL is set but DATABASE_URL is not"
    );
    None
}

pub fn redis_url() -> Option<String> {
    if let Ok(url) = std::env::var("REDIS_URL") {
        return Some(url);
    }
    assert!(
        std::env::var("REQUIRE_REDIS_URL").is_err(),
        "REQUIRE_REDIS_URL is set but REDIS_URL is not"
    );
    None
}
