extern crate self as ferristrace_application;

mod borrowed;
mod catalogue;
mod explore;
mod identity;
mod ingestion;
mod issues;
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
