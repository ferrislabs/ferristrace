#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadLogQuery {
    #[error("a page holds between 1 and {max} log records")]
    Limit { max: u32 },
    #[error("a histogram holds between 1 and {max} buckets")]
    Buckets { max: u32 },
}
