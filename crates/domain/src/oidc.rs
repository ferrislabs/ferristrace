use core::fmt;
use core::future::Future;

use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FlowError {
    #[error("no attempt is waiting under this state")]
    Unknown,
    #[error("the attempt store could not be reached")]
    StoreUnreachable,
}

#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct CodeVerifier(String);

impl CodeVerifier {
    pub fn from_plaintext(plaintext: &str) -> Self {
        Self(plaintext.to_owned())
    }

    pub fn exposed(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for CodeVerifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CodeVerifier(redacted)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PendingFlow {
    pub state: String,
    pub nonce: String,
    pub verifier: CodeVerifier,
    pub redirect_uri: String,
}

impl fmt::Debug for PendingFlow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PendingFlow")
            .field("redirect_uri", &self.redirect_uri)
            .finish_non_exhaustive()
    }
}

pub trait OidcFlows {
    fn begin(&self, flow: &PendingFlow) -> impl Future<Output = Result<(), FlowError>> + Send;

    fn take(&self, state: &str) -> impl Future<Output = Result<PendingFlow, FlowError>> + Send;
}

impl<T: OidcFlows + Sync> OidcFlows for &T {
    fn begin(&self, flow: &PendingFlow) -> impl Future<Output = Result<(), FlowError>> + Send {
        (*self).begin(flow)
    }

    fn take(&self, state: &str) -> impl Future<Output = Result<PendingFlow, FlowError>> + Send {
        (*self).take(state)
    }
}

impl<T: OidcFlows + Send + Sync> OidcFlows for std::sync::Arc<T> {
    fn begin(&self, flow: &PendingFlow) -> impl Future<Output = Result<(), FlowError>> + Send {
        self.as_ref().begin(flow)
    }

    fn take(&self, state: &str) -> impl Future<Output = Result<PendingFlow, FlowError>> + Send {
        self.as_ref().take(state)
    }
}

#[cfg(test)]
mod tests {
    use super::{CodeVerifier, PendingFlow};

    #[test]
    fn a_verifier_never_renders_itself() {
        let verifier = CodeVerifier::from_plaintext("the-verifier");

        assert_eq!(format!("{verifier:?}"), "CodeVerifier(redacted)");
        assert_eq!(verifier.exposed(), "the-verifier");
    }

    #[test]
    fn a_pending_flow_renders_only_where_it_sends_the_browser() {
        let flow = PendingFlow {
            state: "the-state".to_owned(),
            nonce: "the-nonce".to_owned(),
            verifier: CodeVerifier::from_plaintext("the-verifier"),
            redirect_uri: "https://app.example.com/callback".to_owned(),
        };

        let rendered = format!("{flow:?}");

        assert!(rendered.contains("https://app.example.com/callback"));
        assert!(!rendered.contains("the-verifier"));
        assert!(!rendered.contains("the-nonce"));
        assert!(!rendered.contains("the-state"));
    }
}
