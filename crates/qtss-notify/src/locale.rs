//! Locale-aware copy for multi-channel notifications (FAZ 9.4).

/// Picks `turkish` when `preferred` starts with `tr`, otherwise `english`.
pub fn resolve_bilingual(preferred: &str, english: &str, turkish: &str) -> String {
    if preferred.trim().to_lowercase().starts_with("tr") {
        turkish.to_string()
    } else {
        english.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_turkish_when_tr() {
        assert_eq!(
            resolve_bilingual("tr-TR", "en body", "tr body"),
            "tr body"
        );
    }

    #[test]
    fn picks_english_otherwise() {
        assert_eq!(
            resolve_bilingual("en-US", "en body", "tr body"),
            "en body"
        );
    }
}
