#![allow(dead_code)]

pub fn clickhouse_url() -> Option<String> {
    if let Ok(url) = std::env::var("CLICKHOUSE_URL") {
        return Some(url);
    }
    assert!(
        std::env::var("REQUIRE_CLICKHOUSE_URL").is_err(),
        "REQUIRE_CLICKHOUSE_URL is set but CLICKHOUSE_URL is not"
    );
    None
}
