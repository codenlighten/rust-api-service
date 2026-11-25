//! Input validation for request bodies. Each function returns every problem it finds.

use crate::error::FieldError;

pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_BODY_CHARS: usize = 10_000;
pub const MIN_PASSWORD_CHARS: usize = 8;
pub const MAX_PASSWORD_CHARS: usize = 128;
pub const MAX_LIMIT: i64 = 100;
pub const DEFAULT_LIMIT: i64 = 20;

fn err(field: &'static str, message: impl Into<String>) -> FieldError {
    FieldError {
        field,
        message: message.into(),
    }
}

/// Returns the normalised (trimmed, lower-cased) email.
pub fn email(raw: &str) -> Result<String, FieldError> {
    let email = raw.trim().to_lowercase();
    let ok = email.len() <= 254
        && email.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && !domain.contains('@')
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        })
        && !email.chars().any(|c| c.is_whitespace() || c.is_control());
    if ok {
        Ok(email)
    } else {
        Err(err(
            "email",
            "must look like name@example.com and be at most 254 characters",
        ))
    }
}

pub fn password(raw: &str) -> Result<(), FieldError> {
    let n = raw.chars().count();
    if (MIN_PASSWORD_CHARS..=MAX_PASSWORD_CHARS).contains(&n) {
        Ok(())
    } else {
        Err(err(
            "password",
            format!("must be {MIN_PASSWORD_CHARS} to {MAX_PASSWORD_CHARS} characters"),
        ))
    }
}

/// Returns the trimmed title and the body when both are valid.
pub fn note(title: &str, body: &str) -> Result<(String, String), Vec<FieldError>> {
    let mut errors = Vec::new();
    let title = title.trim();
    if title.is_empty() || title.chars().count() > MAX_TITLE_CHARS {
        errors.push(err(
            "title",
            format!("must be 1 to {MAX_TITLE_CHARS} characters"),
        ));
    }
    if body.chars().count() > MAX_BODY_CHARS {
        errors.push(err(
            "body",
            format!("must be at most {MAX_BODY_CHARS} characters"),
        ));
    }
    if errors.is_empty() {
        Ok((title.to_string(), body.to_string()))
    } else {
        Err(errors)
    }
}

pub fn limit(raw: Option<i64>) -> Result<i64, FieldError> {
    match raw {
        None => Ok(DEFAULT_LIMIT),
        Some(n) if (1..=MAX_LIMIT).contains(&n) => Ok(n),
        Some(_) => Err(err("limit", format!("must be 1 to {MAX_LIMIT}"))),
    }
}

pub fn after(raw: Option<i64>) -> Result<i64, FieldError> {
    match raw {
        None => Ok(0),
        Some(n) if n >= 0 => Ok(n),
        Some(_) => Err(err("after", "must be 0 or a note id from 'next_after'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_is_normalised_and_checked() {
        assert_eq!(email("  Ann@Example.COM ").unwrap(), "ann@example.com");
        for bad in [
            "",
            "ann",
            "@example.com",
            "ann@",
            "ann@example",
            "a b@example.com",
            "a@@example.com",
            "a@.example.com",
        ] {
            assert!(email(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn password_length_bounds_count_characters() {
        assert!(password("1234567").is_err());
        assert!(password("12345678").is_ok());
        assert!(password(&"é".repeat(128)).is_ok());
        assert!(password(&"é".repeat(129)).is_err());
    }

    #[test]
    fn note_collects_every_error() {
        let errors = note("   ", &"x".repeat(MAX_BODY_CHARS + 1)).unwrap_err();
        assert_eq!(errors.len(), 2);
        assert_eq!(note("  hi ", "").unwrap().0, "hi");
    }

    #[test]
    fn pagination_bounds() {
        assert_eq!(limit(None).unwrap(), DEFAULT_LIMIT);
        assert!(limit(Some(0)).is_err());
        assert!(limit(Some(MAX_LIMIT + 1)).is_err());
        assert!(after(Some(-1)).is_err());
    }
}
