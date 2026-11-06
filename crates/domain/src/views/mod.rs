mod desk;
#[cfg(test)]
mod desk_tests;
mod entities;
mod errors;
mod ports;
mod value_objects;

pub use desk::ViewDesk;
pub use entities::{NewView, View, ViewChange};
pub use errors::{BadViewDefinition, BadViewName, ViewError};
pub use ports::{Creation, ExplorerViews, Saving};
pub use value_objects::{ViewDefinition, ViewId, ViewName, ViewSignal, Visibility};
