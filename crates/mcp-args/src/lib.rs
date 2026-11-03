use uuid::Uuid;

pub const LOG_SEVERITIES: [&str; 6] = ["trace", "debug", "info", "warn", "error", "fatal"];
pub const ALERT_STATES: [&str; 4] = ["ok", "pending", "firing", "no_data"];
pub const INCIDENT_STATUSES: [&str; 3] = ["open", "resolved", "all"];
pub const AGGREGATIONS: [&str; 8] = ["sum", "avg", "min", "max", "rate", "p50", "p95", "p99"];
pub const DEFAULT_INCIDENT_STATUS: &str = "open";

pub fn given(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|typed| !typed.is_empty())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Absent,
    Is(&'static str),
    Invalid,
}

pub fn choose(value: Option<&str>, allowed: &[&'static str]) -> Choice {
    let Some(typed) = given(value) else {
        return Choice::Absent;
    };
    allowed
        .iter()
        .copied()
        .find(|name| name.eq_ignore_ascii_case(typed))
        .map_or(Choice::Invalid, Choice::Is)
}

pub fn uuid(value: &str) -> Option<Uuid> {
    Uuid::parse_str(given(Some(value))?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_means_absent() {
        assert_eq!(given(None), None);
        assert_eq!(given(Some("  ")), None);
        assert_eq!(given(Some(" checkout ")), Some("checkout"));
    }

    #[test]
    fn choices_are_trimmed_and_case_insensitive() {
        assert_eq!(
            choose(Some(" ERROR "), &LOG_SEVERITIES),
            Choice::Is("error")
        );
        assert_eq!(choose(None, &LOG_SEVERITIES), Choice::Absent);
        assert_eq!(choose(Some("  "), &LOG_SEVERITIES), Choice::Absent);
        assert_eq!(choose(Some("busy"), &ALERT_STATES), Choice::Invalid);
    }

    #[test]
    fn uuids_are_trimmed() {
        let id = "11111111-1111-4111-8111-111111111111";
        assert_eq!(uuid(&format!(" {id}\n")).unwrap().to_string(), id);
        assert!(uuid("nope").is_none());
        assert!(uuid("").is_none());
    }
}
