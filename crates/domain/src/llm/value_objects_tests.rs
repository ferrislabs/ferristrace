use super::errors::{BadCallQuery, BadPrice};
use super::value_objects::{
    CallQuery, MAX_CALL_LIMIT, MAX_MODEL_BYTES, MAX_RATE, ModelPattern, Provider, Rates,
};
use crate::explore::Window;

#[test]
fn a_provider_is_trimmed_lowercased_and_one_word() {
    assert_eq!(
        Provider::try_from("  OpenAI ")
            .expect("a provider")
            .as_str(),
        "openai"
    );
    assert!(Provider::try_from("").is_err());
    assert!(Provider::try_from("open ai").is_err());
    assert!(Provider::try_from(&*"x".repeat(65)).is_err());
}

#[test]
fn a_model_may_end_in_a_star_and_nowhere_else() {
    assert!(ModelPattern::try_from("gpt-4o").is_ok());
    assert!(ModelPattern::try_from("gpt-4*").is_ok());
    assert_eq!(ModelPattern::try_from("gp*t"), Err(BadPrice::Pattern));
    assert_eq!(ModelPattern::try_from("g**"), Err(BadPrice::Pattern));
    assert_eq!(ModelPattern::try_from("gpt 4"), Err(BadPrice::Pattern));
    assert_eq!(
        ModelPattern::try_from(" "),
        Err(BadPrice::Model {
            max: MAX_MODEL_BYTES
        })
    );
}

#[test]
fn a_rate_beyond_the_ceiling_is_refused() {
    assert!(Rates::new(MAX_RATE, MAX_RATE, Some(MAX_RATE)).is_ok());
    assert_eq!(
        Rates::new(1, MAX_RATE + 1, None),
        Err(BadPrice::Rate { max: MAX_RATE })
    );
    assert!(Rates::new(1, 1, Some(MAX_RATE + 1)).is_err());
}

#[test]
fn a_page_holds_between_one_and_the_maximum_calls() {
    let window = Window::new(1, 2).expect("a window");

    assert!(CallQuery::over(window).at_most(MAX_CALL_LIMIT).is_ok());
    assert_eq!(
        CallQuery::over(window).at_most(0),
        Err(BadCallQuery::Limit {
            max: MAX_CALL_LIMIT
        })
    );
    assert!(CallQuery::over(window).at_most(MAX_CALL_LIMIT + 1).is_err());
}
