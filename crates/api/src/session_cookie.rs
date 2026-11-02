use axum::http::HeaderMap;
use axum::http::header::{COOKIE, HeaderValue, USER_AGENT};
use ferristrace_domain::{Agent, SessionToken};

const NAME: &str = "ft_session";
const FLAGS: &str = "Path=/; HttpOnly; SameSite=Lax";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionCookie {
    pub secure: bool,
    pub max_age_seconds: u32,
}

impl Default for SessionCookie {
    fn default() -> Self {
        Self {
            secure: true,
            max_age_seconds: 8 * 60 * 60,
        }
    }
}

pub(crate) fn carried(headers: &HeaderMap) -> Option<SessionToken> {
    named(headers, NAME)
        .map(|value| SessionToken::from_plaintext(&value))
        .filter(SessionToken::looks_like_one)
}

pub(crate) fn opened(token: &SessionToken, cookie: SessionCookie) -> Option<HeaderValue> {
    rendered(
        &format!("{NAME}={}", token.exposed()),
        cookie.max_age_seconds,
        cookie.secure,
    )
}

pub(crate) fn cleared(cookie: SessionCookie) -> Option<HeaderValue> {
    rendered(&format!("{NAME}="), 0, cookie.secure)
}

pub(crate) fn agent(headers: &HeaderMap) -> Option<Agent> {
    headers
        .get(USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .and_then(Agent::new)
}

fn named(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|header| header.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(held, _)| *held == name)
        .map(|(_, value)| value.to_owned())
}

fn rendered(pair: &str, max_age: u32, secure: bool) -> Option<HeaderValue> {
    let mut rendered = format!("{pair}; {FLAGS}; Max-Age={max_age}");
    if secure {
        rendered.push_str("; Secure");
    }
    HeaderValue::from_str(&rendered).ok()
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderMap;
    use axum::http::header::{COOKIE, HeaderValue};
    use ferristrace_domain::SessionToken;

    use super::{SessionCookie, carried, cleared, opened};

    fn headers(values: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for value in values {
            headers.append(COOKIE, HeaderValue::from_str(value).expect("a header"));
        }
        headers
    }

    #[test]
    fn a_session_is_read_from_any_cookie_header() {
        let found = carried(&headers(&["other=1", "ft_session=ft_ss_abc; theme=dark"]));

        assert_eq!(found.as_ref().map(SessionToken::exposed), Some("ft_ss_abc"));
    }

    #[test]
    fn a_cookie_without_the_prefix_is_not_a_session() {
        assert!(carried(&headers(&["ft_session=garbage"])).is_none());
        assert!(carried(&headers(&["theme=dark"])).is_none());
        assert!(carried(&HeaderMap::new()).is_none());
    }

    #[test]
    fn an_opened_cookie_hides_itself_from_scripts_and_from_other_sites() {
        let token = SessionToken::from_plaintext("ft_ss_abc");
        let rendered = opened(
            &token,
            SessionCookie {
                secure: true,
                max_age_seconds: 600,
            },
        )
        .expect("a header");

        assert_eq!(
            rendered.to_str().expect("text"),
            "ft_session=ft_ss_abc; Path=/; HttpOnly; SameSite=Lax; Max-Age=600; Secure"
        );
    }

    #[test]
    fn a_plain_http_deployment_drops_only_the_secure_flag() {
        let token = SessionToken::from_plaintext("ft_ss_abc");
        let rendered = opened(
            &token,
            SessionCookie {
                secure: false,
                max_age_seconds: 600,
            },
        )
        .expect("a header");
        let text = rendered.to_str().expect("text");

        assert!(text.contains("HttpOnly"));
        assert!(text.contains("SameSite=Lax"));
        assert!(!text.contains("Secure"));
    }

    #[test]
    fn a_cleared_cookie_expires_at_once_and_carries_no_value() {
        let rendered = cleared(SessionCookie::default()).expect("a header");
        let text = rendered.to_str().expect("text");

        assert!(text.starts_with("ft_session=;"));
        assert!(text.contains("Max-Age=0"));
    }
}
