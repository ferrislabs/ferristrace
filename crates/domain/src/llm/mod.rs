mod desk;
#[cfg(test)]
mod desk_tests;
mod entities;
mod errors;
mod ports;
mod pricing;
#[cfg(test)]
mod pricing_tests;
mod value_objects;
#[cfg(test)]
mod value_objects_tests;

pub use desk::Llm;
pub use entities::{Call, CallPage, Price, PricedCall, SeenModel, Unpriced};
pub use errors::{BadCallQuery, BadPrice, LlmError};
pub use ports::{LlmReader, ModelPrices};
pub use pricing::PriceBook;
pub use value_objects::{
    CallCursor, CallQuery, Cost, DEFAULT_CALL_LIMIT, MAX_CALL_LIMIT, MAX_MODEL_BYTES,
    MAX_PROVIDER_BYTES, MAX_RATE, MODEL_LIMIT, ModelPattern, PriceDraft, PriceId, Provider, Rates,
};
