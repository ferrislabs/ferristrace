mod catalogue_service;
#[cfg(test)]
mod catalogue_service_tests;
mod entities;
mod errors;
mod ports;
mod value_objects;

pub use catalogue_service::Catalogue;
pub use entities::Project;
pub use errors::{BadProjectName, CatalogueError};
pub use ports::Projects;
pub use value_objects::{Assigned, Created, OwnerTeam, ProjectId, ProjectName, Renamed};
