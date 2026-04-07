//! Helpers for Telegram Bot API <code>parse_mode: HTML</code>.

/// Escapes text for Telegram HTML mode (<code>&amp; &lt; &gt;</code>).
#[must_use]
pub fn escape_telegram_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 8);
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_amp_lt_gt() {
        assert_eq!(
            escape_telegram_html("a < b & c > d"),
            "a &lt; b &amp; c &gt; d"
        );
    }
}
