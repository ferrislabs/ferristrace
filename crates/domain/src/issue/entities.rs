use super::value_objects::{Counted, Decision, Status};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub counted: Counted,
    pub decision: Decision,
    pub regressed: bool,
}

impl Issue {
    pub fn of(counted: Counted, decision: Decision) -> Self {
        let regressed = decision.status == Status::Resolved
            && counted.last_seen_unix_nanos > decision.decided_at_unix_nanos;
        Self {
            counted,
            decision,
            regressed,
        }
    }
}
