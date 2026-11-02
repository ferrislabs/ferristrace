mod derivation;
#[cfg(test)]
mod derivation_tests;
mod desk;
#[cfg(test)]
mod desk_tests;
mod entities;
mod errors;
mod herald;
#[cfg(test)]
mod herald_tests;
mod link;
#[cfg(test)]
mod link_tests;
mod ports;
mod showcase;
#[cfg(test)]
mod showcase_tests;
mod subscriptions;
#[cfg(test)]
mod subscriptions_tests;
mod value_objects;
#[cfg(test)]
mod value_objects_tests;

pub use derivation::{by_error_rate, derive, overall, public_updates, redacted, shown};
pub use desk::StatusDesk;
pub use entities::{
    Announcement, Bulletin, ClaimedLetter, Component, ComponentDraft, ComponentPatch, Heralded,
    Invite, Landing, Letter, LetterBody, Link, LinkNotice, Listed, Observed, PageDraft, PagePatch,
    PublicComponent, PublicIncident, PublicPostmortem, PublicUpdate, PublicView, Saved,
    ServiceLoad, Sheet, StatusPage, Subscriber,
};
pub use errors::{BadStatusPage, LinkRefused, StatusError};
pub use herald::Herald;
pub use link::{LinkClaim, LinkKey, Purpose};
pub use ports::{StatusMailer, StatusOutbox, StatusPages, StatusSubscribers};
pub use showcase::Showcase;
pub use subscriptions::Subscriptions;
pub use value_objects::{
    ANNOUNCE_TICK_SECS, Accent, CONFIRM_LIFETIME_SECS, ComponentId, ComponentName, ComponentStatus,
    DEGRADED_ERROR_PERCENT, DOWN_ERROR_PERCENT, Intro, LetterId, MAX_COMPONENTS, MAX_INTRO_BYTES,
    MAX_SLUG_BYTES, MIN_SLUG_BYTES, Mode, PUBLIC_WINDOW_DAYS, PageName, RESEND_AFTER_SECS,
    SERVICE_WINDOW_SECS, ServiceName, Slug, StatusPageId, SubscriberId, Theme,
    UNSUBSCRIBE_LIFETIME_SECS, UpdateKind,
};
