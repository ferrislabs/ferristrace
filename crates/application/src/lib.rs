extern crate self as ferristrace_application;

mod access;
mod agents;
mod alerts;
#[cfg(test)]
mod alerts_llm_tests;
mod audit;
mod catalogue;
mod explore;
mod export;
mod identity;
mod incidents;
mod ingestion;
mod issues;
mod llm;
mod logs;
mod metrics;
mod oncall;
mod organisation;
mod postmortems;
mod profile;
mod provisioning;
pub mod registry;
mod releases;
mod roster;
mod stack;
mod status;
mod teams;
mod usage;
mod views;
mod workspace;

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
