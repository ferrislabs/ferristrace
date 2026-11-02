use super::value_objects::{CodeVerifier, PendingFlow};

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
