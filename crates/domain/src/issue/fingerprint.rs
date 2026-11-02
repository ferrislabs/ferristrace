use super::value_objects::{Fingerprint, Occurrence};

const HEXISH_FLOOR: usize = 8;

pub fn fingerprint(of: &Occurrence) -> Fingerprint {
    Fingerprint::of(&of.service, &of.exception_type, &of.message)
}

pub fn normalise(message: &str) -> String {
    unquoted(message)
        .split_whitespace()
        .map(|token| {
            let tail = token.rsplit('/').next().unwrap_or(token);
            if hexish(tail) {
                return "x".to_owned();
            }
            undigited(tail)
        })
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn unquoted(message: &str) -> String {
    let mut kept = String::with_capacity(message.len());
    let mut open: Option<char> = None;
    for letter in message.chars() {
        match open {
            Some(quote) => {
                if letter == quote {
                    open = None;
                }
            }
            None => {
                if letter == '\'' || letter == '"' {
                    open = Some(letter);
                    kept.push_str("''");
                } else {
                    kept.push(letter);
                }
            }
        }
    }
    kept
}

fn hexish(token: &str) -> bool {
    let bare: String = token.chars().filter(|letter| *letter != '-').collect();
    bare.len() > HEXISH_FLOOR && bare.chars().all(|letter| letter.is_ascii_hexdigit())
}

fn undigited(token: &str) -> String {
    let mut kept = String::with_capacity(token.len());
    let mut running = false;
    for letter in token.chars() {
        if letter.is_ascii_digit() {
            if !running {
                kept.push('0');
                running = true;
            }
        } else {
            running = false;
            kept.push(letter);
        }
    }
    kept
}
