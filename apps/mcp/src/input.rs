use uuid::Uuid;

use crate::fault::Fault;

pub const MAX_LIMIT: u32 = 200;
pub use ferristrace_mcp_args::{
    AGGREGATIONS, ALERT_STATES, DEFAULT_INCIDENT_STATUS, INCIDENT_STATUSES, LOG_SEVERITIES, given,
};
use ferristrace_mcp_args::{Choice, choose};
const MAX_COMMENT_CHARS: usize = 10_000;
const TRACE_ID_HEX: usize = 32;

pub fn required<'a>(label: &str, value: &'a str) -> Result<&'a str, Fault> {
    given(Some(value)).ok_or_else(|| Fault::argument(format!("{label} is required")))
}

pub fn limit(asked: Option<u32>, default: u32) -> u32 {
    asked.map_or(default, |asked| asked.clamp(1, MAX_LIMIT))
}

pub fn uuid(label: &str, value: &str) -> Result<Uuid, Fault> {
    required(label, value)?;
    ferristrace_mcp_args::uuid(value)
        .ok_or_else(|| Fault::argument(format!("{label} must be a UUID")))
}

pub fn trace_id(value: &str) -> Result<String, Fault> {
    let typed = required("trace_id", value)?;
    if typed.len() == TRACE_ID_HEX && typed.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(typed.to_ascii_lowercase())
    } else {
        Err(Fault::argument(
            "trace_id must be 32 hexadecimal characters, as shown by search_traces",
        ))
    }
}

pub fn choice(
    label: &str,
    value: Option<&str>,
    allowed: &[&'static str],
) -> Result<Option<&'static str>, Fault> {
    match choose(value, allowed) {
        Choice::Absent => Ok(None),
        Choice::Is(name) => Ok(Some(name)),
        Choice::Invalid => Err(Fault::argument(format!(
            "{label} must be one of {}, not {:?}",
            allowed.join(", "),
            given(value).unwrap_or_default()
        ))),
    }
}

pub fn comment(value: &str) -> Result<&str, Fault> {
    let typed = required("text", value)?;
    if typed.chars().count() > MAX_COMMENT_CHARS {
        return Err(Fault::argument(format!(
            "text is longer than {MAX_COMMENT_CHARS} characters"
        )));
    }
    Ok(typed)
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
    fn required_names_the_missing_argument() {
        assert_eq!(
            required("service", " ").unwrap_err().to_string(),
            "service is required"
        );
    }

    #[test]
    fn limits_are_clamped_to_the_row_cap() {
        assert_eq!(limit(None, 50), 50);
        assert_eq!(limit(Some(0), 50), 1);
        assert_eq!(limit(Some(5000), 50), MAX_LIMIT);
        assert_eq!(limit(Some(7), 50), 7);
    }

    #[test]
    fn ids_must_be_well_formed() {
        let id = "11111111-1111-4111-8111-111111111111";
        assert_eq!(uuid("incident_id", id).unwrap().to_string(), id);
        assert_eq!(
            uuid("incident_id", "nope").unwrap_err().to_string(),
            "incident_id must be a UUID"
        );
        assert!(uuid("incident_id", "").is_err());
    }

    #[test]
    fn trace_ids_are_32_hex_and_lowercased() {
        let id = "ABCDEF0123456789abcdef0123456789";
        assert_eq!(trace_id(id).unwrap(), id.to_ascii_lowercase());
        assert!(trace_id("abc").is_err());
        assert!(trace_id(&"g".repeat(32)).is_err());
        assert!(trace_id("../../etc").is_err());
    }

    #[test]
    fn choices_are_case_insensitive_and_listed_on_error() {
        assert_eq!(
            choice("severity", Some("ERROR"), &LOG_SEVERITIES).unwrap(),
            Some("error")
        );
        assert_eq!(choice("severity", None, &LOG_SEVERITIES).unwrap(), None);
        let message = choice("state", Some("busy"), &ALERT_STATES)
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("ok, pending, firing, no_data"),
            "{message}"
        );
    }

    #[test]
    fn comments_are_bounded() {
        assert_eq!(comment(" hi ").unwrap(), "hi");
        assert!(comment("").is_err());
        assert!(comment(&"x".repeat(MAX_COMMENT_CHARS + 1)).is_err());
    }
}
