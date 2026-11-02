use super::entities::{Call, Price};
use super::value_objects::Cost;

const NANOS_PER_MILLI: u64 = 1_000_000;

#[derive(Debug, Clone, Copy)]
pub struct PriceBook<'a> {
    prices: &'a [Price],
}

impl<'a> PriceBook<'a> {
    pub fn new(prices: &'a [Price]) -> Self {
        Self { prices }
    }

    pub fn find(&self, provider: &str, model: &str, at_ms: u64) -> Option<&'a Price> {
        self.prices
            .iter()
            .filter(|price| price.provider.is(provider) && price.effective_from_ms <= at_ms)
            .filter_map(|price| {
                let (exact, length) = price.model.matches(model)?;
                Some(((exact, length, price.effective_from_ms), price))
            })
            .max_by_key(|(rank, _)| *rank)
            .map(|(_, price)| price)
    }

    pub fn of_call(&self, call: &Call) -> Option<&'a Price> {
        let at_ms = call.time_unix_nanos / NANOS_PER_MILLI;
        [&call.response_model, &call.request_model]
            .into_iter()
            .filter(|model| !model.is_empty())
            .find_map(|model| self.find(&call.provider, model, at_ms))
    }

    pub fn cost(&self, call: &Call) -> Option<Cost> {
        self.of_call(call).map(|price| {
            price.rates.cost(
                call.input_tokens,
                call.output_tokens,
                call.cached_input_tokens,
            )
        })
    }
}
