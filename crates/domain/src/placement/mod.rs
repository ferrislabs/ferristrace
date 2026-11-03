mod errors;
mod ports;
mod value_objects;
#[cfg(test)]
mod value_objects_tests;

pub use errors::BadPlacement;
pub use ports::PlacementReader;
pub use value_objects::{
    CLUSTER_KEY, MAX_PLACEMENT_BYTES, MAX_PLACEMENTS, NAMESPACE_KEY, NODE_KEY, POD_KEY, Placement,
    PlacementName, Placements, SeenPod, SeenWorkload, ServiceSite, Sighting, Workload,
    WorkloadKind,
};
