use crate::access::ReadRefusal;
use crate::metrics::BadMetricQuery;
use crate::organisation::WhoamiError;
use crate::store::StoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadRule {
    #[error("a rule needs a name of at most {max} bytes")]
    Name { max: usize },
    #[error("a window lasts between {min} and {max} seconds")]
    Window { min: u64, max: u64 },
    #[error("a pending duration lasts at most {max} seconds")]
    Pending { max: u64 },
    #[error("a threshold is a finite number")]
    Threshold,
    #[error("an operation is named within a service, so a rule naming one names its service")]
    OperationWithoutService,
    #[error("a metric rule names its metric")]
    MetricName,
    #[error("an attribute key cannot be blank")]
    BlankKey,
    #[error("a metric rule groups and filters by at most {max} attribute keys")]
    GroupBy { max: usize },
    #[error("an llm filter value cannot be blank")]
    LlmFilter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadChannel {
    #[error("a channel needs a name of at most {max} bytes")]
    Name { max: usize },
    #[error("an email channel names at least one address")]
    NoAddress,
    #[error("an email channel names at most {max} addresses")]
    TooManyAddresses { max: usize },
    #[error("an address of the channel is not an email address")]
    Address,
    #[error("a webhook is an http or https url with a host and no credentials")]
    Url,
    #[error("a channel keeps its kind; create another one instead")]
    KindChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadSilence {
    #[error("a silence lifts at a moment still to come")]
    InThePast,
    #[error("a silence lasts at most {max_days} days")]
    TooLong { max_days: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProbeError {
    #[error("a store could not be reached")]
    Unreachable,
    #[error("no reader holds that signal on this deployment")]
    Unavailable,
    #[error(transparent)]
    Inapplicable(#[from] BadMetricQuery),
}

impl From<StoreError> for ProbeError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Unreachable => Self::Unreachable,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct DeliveryError(pub Box<str>);

impl DeliveryError {
    pub fn new(reason: &str) -> Self {
        Self(reason.into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AlertError {
    #[error("neither a live session nor a live read key carries this credential")]
    NoSession,
    #[error("this reader holds no membership of that organisation")]
    NotYours,
    #[error("this agent holds no scope that allows this")]
    OutOfScope,
    #[error("alert rules and channels are an administrator's to change")]
    NotYoursToChange,
    #[error("no rule of this organisation carries that id")]
    NoSuchRule,
    #[error("no channel of this organisation carries that id")]
    NoSuchChannel,
    #[error("no project of this organisation carries that id")]
    NoSuchProject,
    #[error("a rule names a channel this organisation does not hold")]
    UnknownChannel,
    #[error("a rule pages the on-call of a schedule this organisation does not hold")]
    UnknownSchedule,
    #[error("that name is already taken in this organisation")]
    NameTaken,
    #[error(
        "a webhook secret is kept sealed at rest, and this deployment has no secrets.at_rest_key"
    )]
    NoSecretKeeper,
    #[error("this deployment does not send webhooks to private, loopback or link-local addresses")]
    PrivateTarget,
    #[error(transparent)]
    Rule(#[from] BadRule),
    #[error(transparent)]
    Channel(#[from] BadChannel),
    #[error(transparent)]
    Silence(#[from] BadSilence),
    #[error(transparent)]
    Query(#[from] BadMetricQuery),
    #[error("no reader holds that signal on this deployment")]
    SignalUnavailable,
    #[error("a store could not be reached")]
    StoreUnreachable,
}

impl From<WhoamiError> for AlertError {
    fn from(error: WhoamiError) -> Self {
        match error {
            WhoamiError::NoSession => Self::NoSession,
            WhoamiError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<StoreError> for AlertError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Unreachable => Self::StoreUnreachable,
        }
    }
}

impl From<ReadRefusal> for AlertError {
    fn from(error: ReadRefusal) -> Self {
        match error {
            ReadRefusal::Unknown => Self::NoSession,
            ReadRefusal::NotYours => Self::NotYours,
            ReadRefusal::OutOfScope => Self::OutOfScope,
            ReadRefusal::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<ProbeError> for AlertError {
    fn from(error: ProbeError) -> Self {
        match error {
            ProbeError::Unreachable => Self::StoreUnreachable,
            ProbeError::Unavailable => Self::SignalUnavailable,
            ProbeError::Inapplicable(query) => Self::Query(query),
        }
    }
}
