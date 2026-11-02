use super::value_objects::DecompressedBytes;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidId {
    #[error("an identifier of {expected} bytes was given {actual}")]
    Length { expected: usize, actual: usize },
    #[error("the identifier is all zero")]
    Zero,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SinkError {
    #[error("the span sink is unavailable")]
    Unavailable,
    #[error("a span timestamp falls outside the range the span store can hold")]
    TimestampOutOfRange,
    #[error("the sink accepts at most {limit} spans per batch")]
    BatchTooLarge { limit: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IngestError {
    #[error(transparent)]
    UnknownKey(#[from] UnknownKey),
    #[error("the tenant has used its allowance of {} bytes", allowance.get())]
    Capped { allowance: DecompressedBytes },
    #[error(transparent)]
    Sink(#[from] SinkError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the ingest key matches no tenant")]
pub struct UnknownKey;
