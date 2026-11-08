mod alert_book;
#[cfg(test)]
mod alert_book_tests;
mod channel_book;
#[cfg(test)]
mod channel_book_tests;
mod courier;
#[cfg(test)]
mod courier_tests;
mod culprits;
#[cfg(test)]
mod culprits_tests;
mod design;
mod entities;
mod errors;
mod evaluator;
#[cfg(test)]
mod evaluator_tests;
mod llm_signal;
#[cfg(test)]
mod llm_signal_tests;
mod ports;
mod state_machine;
#[cfg(test)]
mod state_machine_tests;
mod value_objects;
#[cfg(test)]
mod value_objects_tests;

pub use alert_book::{AlertBook, HISTORY_LIMIT, PREVIEW_POINTS};
pub use channel_book::ChannelBook;
pub use courier::{Courier, DRAIN_BATCH, RETRY_AFTER_SECS, deliver, settlement_after};
pub use culprits::{
    CulpritPod, Culprits, MAX_CULPRIT_FACETS, MAX_CULPRIT_LABELS, MAX_CULPRIT_PODS,
};
pub use design::{
    Colors, CustomField, Design, Fact, MAX_CUSTOM_FIELDS, MAX_PINGS, Mentions, Shown, Template,
    colour, snowflake, thumbnail,
};
pub use entities::{
    AlertEvent, Channel, ChannelChange, ChannelView, Claimed, Delivery, Dispatch, Drained,
    IssuedChannel, Notification, Observation, Overall, Preview, Rule, RuleDraft, RulePatch,
    RuleState, SeriesState, Settlement, Tick, Transition, Watched,
};
pub use errors::{AlertError, BadChannel, BadDesign, BadRule, DeliveryError, ProbeError};
pub use evaluator::{Evaluator, TICK_SECS};
pub use ports::{
    AlertChannels, AlertDuty, AlertEvents, AlertMailer, AlertOutbox, AlertRules, AlertStates,
    CulpritReader, SignalProbe, WebhookSender,
};
pub use state_machine::next_state;
pub use value_objects::{
    AlertSeverity, AlertStatus, ChannelId, ChannelKind, ChannelName, Chat, Comparison, Condition,
    Definition, DefinitionParts, DeliveryStatus, Destination, Duty, EmailList, EventId,
    HISTORY_DAYS, Labels, LlmGroup, LlmMeasure, LlmSignal, LogSignal, MAX_ADDRESSES,
    MAX_CHANNEL_NAME_BYTES, MAX_PENDING_SECS, MAX_RULE_NAME_BYTES, MAX_WINDOW_SECS,
    MIN_WINDOW_SECS, MetricSignal, NoDataPolicy, NotificationId, RuleId, RuleName, Signal,
    TraceMeasure, TraceSignal, WEBHOOK_SECRET_BYTES, WebhookSecret, WebhookUrl,
};
