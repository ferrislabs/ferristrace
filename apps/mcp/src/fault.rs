use crate::window::BadWindow;

#[derive(Debug, thiserror::Error)]
pub enum Fault {
    #[error("no access token reached the tool")]
    NoToken,
    #[error("no organisation in the URL path")]
    NoOrganisation,
    #[error(transparent)]
    Window(#[from] BadWindow),
    #[error("{0}")]
    Argument(String),
    #[error("the FerrisTrace API is unreachable")]
    Unreachable,
    #[error("the FerrisTrace API answered with something that is not JSON")]
    Unreadable,
    #[error("the FerrisTrace API answer is larger than {limit} bytes; narrow the request")]
    TooLarge { limit: usize },
    #[error("FerrisTrace API {status}: {message}")]
    Refused { status: u16, message: String },
}

impl Fault {
    pub fn argument(message: impl Into<String>) -> Self {
        Self::Argument(message.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_carries_the_status_and_the_api_message() {
        let fault = Fault::Refused {
            status: 403,
            message: "this agent holds no scope that allows this".to_owned(),
        };
        assert_eq!(
            fault.to_string(),
            "FerrisTrace API 403: this agent holds no scope that allows this"
        );
    }

    #[test]
    fn a_bad_window_reads_as_the_window_message() {
        let fault = Fault::from(BadWindow("2w".to_owned()));
        assert!(fault.to_string().starts_with("window must be one of"));
    }
}
