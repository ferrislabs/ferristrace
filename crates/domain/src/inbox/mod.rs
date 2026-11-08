mod desk;
#[cfg(test)]
mod desk_tests;
mod entities;
mod errors;
mod ports;
mod value_objects;
#[cfg(test)]
mod value_objects_tests;

pub use desk::InboxDesk;
pub use entities::{InboxItem, InboxItemDraft};
pub use errors::InboxError;
pub use ports::InboxStore;
pub use value_objects::{InboxItemId, InboxItemKind, PAGE_LIMIT, RETENTION_DAYS};
