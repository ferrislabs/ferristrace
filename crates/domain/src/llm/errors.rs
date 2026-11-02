use crate::access::ReadRefusal;
use crate::organisation::WhoamiError;
use crate::store::StoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadPrice {
    #[error("a provider is named in at most {max} bytes")]
    Provider { max: usize },
    #[error("a model is named in at most {max} bytes")]
    Model { max: usize },
    #[error("a model name carries no blank, and `*` only as its last character")]
    Pattern,
    #[error("a price is a whole number of micro-dollars per million tokens, at most {max}")]
    Rate { max: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadCallQuery {
    #[error("a page holds between 1 and {max} calls")]
    Limit { max: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LlmError {
    #[error("neither a live session nor a live read key carries this credential")]
    NoSession,
    #[error("this reader holds no membership of that organisation")]
    NotYours,
    #[error("model prices are an administrator's to change")]
    NotYoursToChange,
    #[error("no price of this organisation carries that id")]
    NoSuchPrice,
    #[error("that model already has a price from that date")]
    PriceTaken,
    #[error("a store could not be reached")]
    StoreUnreachable,
}

impl From<WhoamiError> for LlmError {
    fn from(error: WhoamiError) -> Self {
        match error {
            WhoamiError::NoSession => Self::NoSession,
            WhoamiError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<StoreError> for LlmError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Unreachable => Self::StoreUnreachable,
        }
    }
}

impl From<ReadRefusal> for LlmError {
    fn from(error: ReadRefusal) -> Self {
        match error {
            ReadRefusal::Unknown => Self::NoSession,
            ReadRefusal::NotYours => Self::NotYours,
            ReadRefusal::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}
