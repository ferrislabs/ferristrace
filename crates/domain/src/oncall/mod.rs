mod ack;
#[cfg(test)]
mod ack_tests;
mod desk;
#[cfg(test)]
mod desk_tests;
mod entities;
mod errors;
mod escalation;
#[cfg(test)]
mod escalation_tests;
#[cfg(test)]
pub(crate) mod fixtures;
mod paging;
#[cfg(test)]
mod paging_tests;
mod ports;
mod rota;
#[cfg(test)]
mod rota_tests;
mod rotation;
#[cfg(test)]
mod rotation_tests;
mod value_objects;
#[cfg(test)]
mod value_objects_tests;

pub use ack::{AckClaim, AckKey};
pub use desk::PageDesk;
pub use entities::{
    ClaimedNotice, Escalation, Notice, OnCallNow, Origin, Override, Page, PageDispatch, PageSource,
    PageView, Paged, Rostered, Rotation, Schedule, ScheduleDraft, SchedulePatch, Shift,
};
pub use errors::{AckRefused, BadSchedule, OncallError};
pub use escalation::{escalated, next};
pub use paging::{Pager, Paging};
pub use ports::{Pages, Schedules};
pub use rota::Rota;
pub use rotation::{on_call, shift};
pub use value_objects::{
    ACK_LIFETIME_SECS, Day, ESCALATE_AFTER_SECS, Handoff, MAX_LENGTH_DAYS, MAX_ROTATION,
    MAX_SCHEDULE_NAME_BYTES, MIN_LENGTH_DAYS, NoticeId, OverrideId, PAGE_TICK_SECS, PageId,
    PageScope, PageState, RotationLength, ScheduleId, ScheduleName, TimeOfDay, Timezone, Weekday,
};
