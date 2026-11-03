#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    NoPermittingPolicy,
    PdpTimeout,
    PdpUnavailable,
    PdpMalformed,
    PipUnavailable,
    UnknownTool,
    InvalidTarget,
    MethodNotAllowed,
    HeaderMismatch,
    UnknownTarget,
    InvalidRequest,
    UpstreamUnavailable,
    ExchangeFailed,
}

impl Reason {
    pub fn code(self) -> &'static str {
        match self {
            Self::NoPermittingPolicy => "no_permitting_policy",
            Self::PdpTimeout => "pdp_timeout",
            Self::PdpUnavailable => "pdp_unavailable",
            Self::PdpMalformed => "pdp_malformed",
            Self::PipUnavailable => "pip_unavailable",
            Self::UnknownTool => "unknown_tool",
            Self::InvalidTarget => "invalid_target",
            Self::MethodNotAllowed => "method_not_allowed",
            Self::HeaderMismatch => "header_mismatch",
            Self::UnknownTarget => "unknown_target",
            Self::InvalidRequest => "invalid_request",
            Self::UpstreamUnavailable => "upstream_unavailable",
            Self::ExchangeFailed => "exchange_failed",
        }
    }

    pub fn explanation(self) -> &'static str {
        match self {
            Self::NoPermittingPolicy => "no policy of your organisation permits this call",
            Self::PdpTimeout => "the policy service did not answer in time",
            Self::PdpUnavailable => "the policy service is unavailable",
            Self::PdpMalformed => "the policy service gave an answer that could not be read",
            Self::PipUnavailable => "a fact the policies need could not be read from FerrisTrace",
            Self::UnknownTool => "there is no such tool",
            Self::InvalidTarget => "the call names something that cannot be checked",
            Self::MethodNotAllowed => "this gateway does not serve that method",
            Self::HeaderMismatch => {
                "the Mcp-Method or Mcp-Name header does not match the request body"
            }
            Self::UnknownTarget => {
                "FerrisTrace does not know the incident or service the call names"
            }
            Self::InvalidRequest => "the request is not a valid MCP request",
            Self::UpstreamUnavailable => "the MCP server did not answer usably",
            Self::ExchangeFailed => "the token for FerrisTrace could not be obtained",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_reason_has_a_distinct_snake_case_code_and_a_message() {
        let all = [
            Reason::NoPermittingPolicy,
            Reason::PdpTimeout,
            Reason::PdpUnavailable,
            Reason::PdpMalformed,
            Reason::PipUnavailable,
            Reason::UnknownTool,
            Reason::InvalidTarget,
            Reason::MethodNotAllowed,
            Reason::HeaderMismatch,
            Reason::UnknownTarget,
            Reason::InvalidRequest,
            Reason::UpstreamUnavailable,
            Reason::ExchangeFailed,
        ];
        let mut codes: Vec<_> = all.iter().map(|reason| reason.code()).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), all.len());
        for reason in all {
            assert!(
                reason
                    .code()
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_')
            );
            assert!(!reason.explanation().is_empty());
        }
    }
}
