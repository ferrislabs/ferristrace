mod context;
#[cfg(test)]
mod context_tests;
mod desk;
#[cfg(test)]
mod desk_tests;
mod entities;
mod errors;
mod followers;
mod insights;
#[cfg(test)]
mod insights_tests;
mod narration;
#[cfg(test)]
mod narration_tests;
mod ports;
mod value_objects;
#[cfg(test)]
mod value_objects_tests;

pub use context::{
    FIGURES_MS, IncidentContext, LOOKBACK_MS, LeftOut, MAX_DEPLOYS, MAX_ISSUES, MAX_RESOURCES,
    MAX_TRACES, Surroundings, context_span_ms,
};
pub use desk::{DEFAULT_SEVERITY, IncidentDesk};
pub use entities::{
    AlertRef, Attachment, Author, Briefed, Change, Chronicle, Dossier, FORMER_MEMBER, Incident,
    IncidentDraft, IncidentFilter, IncidentPatch, Person, TimelineEntry,
};
pub use errors::{BadIncident, IncidentError};
pub use followers::Followers;
pub use insights::{DEFAULT_WINDOW_DAYS, Group, Insights, MAX_WINDOW_DAYS, Timing};
pub use narration::{ANNOUNCED, ClaimedIncidentNotice, IncidentNotice, IncidentOutbox, Narrator};
pub use ports::{IncidentAttachments, IncidentTimeline, Incidents};
pub use value_objects::{
    AttachmentId, AttachmentKind, AttachmentLabel, CommentBody, EntryKind, IncidentId,
    IncidentScope, IncidentSeverity, IncidentStatus, IncidentTitle, MAX_COMMENT_BYTES,
    MAX_LABEL_BYTES, MAX_REFERENCE_BYTES, MAX_TITLE_BYTES, Reference, TimelineId,
};
