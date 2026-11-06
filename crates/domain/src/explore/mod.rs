mod errors;
mod explore_service;
#[cfg(test)]
mod explore_service_tests;
mod ports;
mod query;
mod value_objects;

pub use errors::{BadQuery, BadWindow, ExploreError};
pub use explore_service::Explore;
pub use ports::SpanReader;
pub use query::{
    AttributeKey, Clause, Expr, Field, MAX_CONDITIONS, MAX_DEPTH, MAX_LIST, Number, Op, Query,
    Scalar, Test, Value,
};
pub use value_objects::{
    AttributeFilter, DurationRange, FACET_VALUES, FacetFilter, FacetValues, Facets, ListedSpan,
    MAX_ATTRIBUTES, MAX_FACETS, MAX_VALUES, SeenAttribute, SeenValue, ServiceFilter, ServicePoint,
    ServiceSeries, ServiceSummary, SpanFilter, SpanOrder, TraceFilter, TraceOrder, TraceSummary,
    Window,
};
