use crate::access::ReadRefusal;
use crate::organisation::WhoamiError;
use crate::store::StoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadSchedule {
    #[error("a schedule needs a name of at most {max} bytes")]
    Name { max: usize },
    #[error("a schedule names an IANA timezone, like Europe/Paris or UTC")]
    Timezone,
    #[error("a handoff happens at a time of day written HH:MM")]
    Time,
    #[error("a handoff happens on a weekday, monday to sunday")]
    Weekday,
    #[error("a rotation lasts between {min} and {max} days")]
    Length { min: u8, max: u8 },
    #[error("a schedule starts on a date written YYYY-MM-DD")]
    Day,
    #[error("a rotation names each person once and at most {max} people")]
    Rotation { max: usize },
    #[error("an override ends after it starts")]
    Window,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AckRefused {
    #[error("this acknowledgement link was not signed by this deployment")]
    Tampered,
    #[error("this acknowledgement link has expired")]
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OncallError {
    #[error("neither a live session nor a live read key carries this credential")]
    NoSession,
    #[error("this reader holds no membership of that organisation")]
    NotYours,
    #[error("this agent holds no scope that allows this")]
    OutOfScope,
    #[error("on-call schedules are an administrator's to change")]
    NotYoursToChange,
    #[error("a read key reads schedules and pages; acknowledging takes a session")]
    ReadOnly,
    #[error("a page is acknowledged by the person it targets or an administrator")]
    NotYourPage,
    #[error("no team of this organisation carries that id")]
    NoSuchTeam,
    #[error("no schedule of this organisation carries that id")]
    NoSuchSchedule,
    #[error("no override of this schedule carries that id")]
    NoSuchOverride,
    #[error("no page of this organisation carries that id")]
    NoSuchPage,
    #[error("a rotation is made of members of the schedule's team")]
    NotInTeam,
    #[error("an override hands the schedule to a member of the organisation")]
    NotAMember,
    #[error("that name is already taken in this team")]
    NameTaken,
    #[error("acknowledging by link needs secrets.at_rest_key on this deployment")]
    NoAckKey,
    #[error(transparent)]
    Schedule(#[from] BadSchedule),
    #[error(transparent)]
    Ack(#[from] AckRefused),
    #[error("a store could not be reached")]
    StoreUnreachable,
}

impl From<WhoamiError> for OncallError {
    fn from(error: WhoamiError) -> Self {
        match error {
            WhoamiError::NoSession => Self::NoSession,
            WhoamiError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<StoreError> for OncallError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Unreachable => Self::StoreUnreachable,
        }
    }
}

impl From<ReadRefusal> for OncallError {
    fn from(error: ReadRefusal) -> Self {
        match error {
            ReadRefusal::Unknown => Self::NoSession,
            ReadRefusal::NotYours => Self::NotYours,
            ReadRefusal::OutOfScope => Self::OutOfScope,
            ReadRefusal::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}
