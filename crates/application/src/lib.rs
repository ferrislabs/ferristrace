extern crate self as ferristrace_application;

mod access;
mod alerts;
mod audit;
mod borrowed;
mod catalogue;
mod explore;
mod export;
mod identity;
mod incidents;
mod ingestion;
mod issues;
mod logs;
mod metrics;
mod organisation;
mod profile;
mod provisioning;
pub mod registry;
mod roster;
mod stack;
mod teams;
mod usage;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use alerts::SignalReaders;
pub use ferristrace_macros::{repository, transactional};
pub use ingestion::Ingestion;
pub use issues::IssueRuling;
pub use stack::{Adapters, Ferristrace, Stack};

#[cfg(test)]
mod fakes;
#[cfg(test)]
mod recording;
#[cfg(test)]
mod tests;
