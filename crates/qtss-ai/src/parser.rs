//! Extract and validate JSON decision blobs from LLM output (provider-agnostic).

use serde_json::{json, Value};

use crate::error::{AiError, AiResult};

const TACTICAL_DIRECTIONS: &[&str] = &[
    "strong_buy",
    "buy",
    "neutral",
    "sell",
    "strong_sell",
    "no_trade",
];

const OPERATIONAL_ACTIONS: &[&str] = &[
    "keep",
    "tighten_stop",
    "widen_stop",
    "activate_trailing",
    "deactivate_trailing",
    "partial_close",
    "full_close",
    "add_to_position",
];

/// Case-insensitive ASCII substring search (for ```json / ```JSON).
fn find_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    let nb = needle.as_bytes();
    let hb = haystack.as_bytes();
    'outer: for i in 0..=hb.len().saturating_sub(nb.len()) {
        for j in 0..nb.len() {
            if hb[i + j].to_ascii_lowercase() != nb[j].to_ascii_lowercase() {
                continue 'outer;
            }
        }
        return Some(i);
    }
    None
}

/// Extract first `{`…`}` that parses as a JSON object: brace matching, then scan `}` if truncated.
fn try_extract_json_object(s: &str) -> AiResult<String> {
    let s = s.trim();
    let start = s.find('{').ok_or_else(|| AiError::parse("no JSON object start"))?;
    let slice = &s[start..];
    if let Some(end) = find_matching_brace(slice) {
        let cand = slice[..=end].to_string();
        if serde_json::from_str::<Value>(&cand).is_ok() {
            return Ok(cand);
        }
    }
    // Gemini / long outputs: missing closing ``` fence or `max_tokens` cut mid-object — try each `}`.
    for (i, ch) in slice.char_indices() {
        if ch != '}' {
            continue;
        }
        let end = i + ch.len_utf8();
        let cand = &slice[..end];
        if let Ok(v) = serde_json::from_str::<Value>(cand) {
            if v.is_object() {
                return Ok(cand.to_string());
            }
        }
    }
    Err(AiError::parse("unbalanced JSON braces"))
}

/// Pulls a JSON object from markdown fences or the first `{`…`}` span.
pub fn extract_json_block(raw: &str) -> AiResult<String> {
    let t = raw.trim();

    // ```json … ``` (closing fence optional — models often omit it when truncated).
    if let Some(idx) = find_case_insensitive(t, "```json") {
        let rest = t[idx + "```json".len()..].trim_start();
        let content = if let Some(close) = rest.find("```") {
            rest[..close].trim()
        } else {
            rest.trim()
        };
        if let Ok(s) = try_extract_json_object(content) {
            return Ok(s);
        }
    }

    // Generic ``` fence (optional language line).
    if let Some(idx) = t.find("```") {
        let mut rest = t[idx + 3..].trim_start();
        if !rest.starts_with('{') {
            if let Some(nl) = rest.find('\n') {
                let lang = rest[..nl].trim().to_lowercase();
                if lang.is_empty() || lang == "json" {
                    rest = rest[nl + 1..].trim_start();
                }
            }
        }
        let content = if let Some(close) = rest.find("```") {
            rest[..close].trim()
        } else {
            rest.trim()
        };
        if !content.is_empty() && content.contains('{') {
            if let Ok(s) = try_extract_json_object(content) {
                return Ok(s);
            }
        }
    }

    try_extract_json_object(t)
}

fn find_matching_brace(s: &str) -> Option<usize> {
    let mut depth = 0_i32;
    let mut in_str = false;
    let mut esc = false;
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Pull a JSON string value for `key` (first occurrence). Handles `\"` escapes; returns `None` if the string is unclosed (truncated).
fn parse_json_string_field<'a>(raw: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\"");
    let idx = raw.find(&needle)?;
    let mut s = &raw[idx + needle.len()..];
    s = s.trim_start();
    if !s.starts_with(':') {
        return None;
    }
    s = s[1..].trim_start();
    if !s.starts_with('"') {
        return None;
    }
    s = &s[1..];
    let bytes = s.as_bytes();
    let mut i = 0_usize;
    let mut esc = false;
    while i < bytes.len() {
        let b = bytes[i];
        if esc {
            esc = false;
            i += 1;
            continue;
        }
        if b == b'\\' {
            esc = true;
            i += 1;
            continue;
        }
        if b == b'"' {
            return Some(&s[..i]);
        }
        let ch = s[i..].chars().next()?;
        i += ch.len_utf8();
    }
    None
}

/// Pull a JSON number for `key` (first occurrence).
fn parse_json_number_field(raw: &str, key: &str) -> Option<f64> {
    let needle = format!("\"{key}\"");
    let idx = raw.find(&needle)?;
    let mut s = raw[idx + needle.len()..].trim_start();
    if !s.starts_with(':') {
        return None;
    }
    s = s[1..].trim_start();
    let end = s
        .find(|c: char| c.is_ascii_whitespace() || c == ',' || c == '}')
        .unwrap_or(s.len());
    if end == 0 {
        return None;
    }
    s[..end].parse().ok()
}

/// Recover a minimal tactical object when the model truncates mid-JSON (e.g. max_output_tokens while typing `position_size_multiplier`).
fn salvage_truncated_tactical_json(raw: &str) -> Option<String> {
    let direction_raw = parse_json_string_field(raw, "direction")?;
    let direction = direction_raw.trim().to_lowercase().replace(' ', "_");
    if !TACTICAL_DIRECTIONS.contains(&direction.as_str()) {
        return None;
    }
    let confidence = parse_json_number_field(raw, "confidence")?;
    if !(0.0..=1.0).contains(&confidence) {
        return None;
    }
    let mut map = serde_json::Map::new();
    map.insert("direction".into(), Value::String(direction));
    map.insert("confidence".into(), json!(confidence));
    if let Some(x) = parse_json_number_field(raw, "stop_loss_pct") {
        map.insert("stop_loss_pct".into(), json!(x));
    }
    if let Some(x) = parse_json_number_field(raw, "take_profit_pct") {
        map.insert("take_profit_pct".into(), json!(x));
    }
    if let Some(x) = parse_json_number_field(raw, "position_size_multiplier") {
        map.insert("position_size_multiplier".into(), json!(x));
    }
    if let Some(x) = parse_json_number_field(raw, "entry_price_hint") {
        map.insert("entry_price_hint".into(), json!(x));
    }
    if let Some(s) = parse_json_string_field(raw, "reasoning") {
        if !s.is_empty() {
            map.insert("reasoning".into(), Value::String(s.to_string()));
        }
    }
    Some(Value::Object(map).to_string())
}

fn require_direction(v: &Value) -> AiResult<String> {
    let d = v
        .get("direction")
        .and_then(|x| x.as_str())
        .ok_or_else(|| AiError::parse("missing direction"))?;
    let d = d.trim().to_lowercase().replace(' ', "_");
    if TACTICAL_DIRECTIONS.contains(&d.as_str()) {
        Ok(d)
    } else {
        Err(AiError::parse(format!("invalid direction: {d}")))
    }
}

fn require_confidence(v: &Value) -> AiResult<f64> {
    let c = v.get("confidence").and_then(|x| x.as_f64()).ok_or_else(|| AiError::parse("missing confidence"))?;
    if !(0.0..=1.0).contains(&c) {
        return Err(AiError::parse("confidence must be 0.0..=1.0"));
    }
    Ok(c)
}

fn clamp_multiplier(v: &Value) -> AiResult<()> {
    if let Some(m) = v.get("position_size_multiplier").and_then(|x| x.as_f64()) {
        if !(0.0..=2.0).contains(&m) {
            return Err(AiError::parse("position_size_multiplier must be 0.0..=2.0"));
        }
    }
    Ok(())
}

/// Normalized tactical JSON (`direction`, `confidence`, optional multiplier / SL / TP / reasoning).
pub fn parse_tactical_decision(raw: &str) -> AiResult<Value> {
    let s = match extract_json_block(raw) {
        Ok(s) => s,
        Err(e) => salvage_truncated_tactical_json(raw).ok_or(e)?,
    };
    let v: Value = serde_json::from_str(&s).map_err(|e| AiError::parse(e.to_string()))?;
    let dir = require_direction(&v)?;
    let conf = require_confidence(&v)?;
    clamp_multiplier(&v)?;
    let mult = v
        .get("position_size_multiplier")
        .and_then(|x| x.as_f64())
        .unwrap_or(1.0);
    let mut out = json!({
        "direction": dir,
        "confidence": conf,
        "position_size_multiplier": mult,
    });
    if let Some(x) = v.get("entry_price_hint").and_then(|x| x.as_f64()) {
        out["entry_price_hint"] = json!(x);
    }
    if let Some(x) = v.get("stop_loss_pct").and_then(|x| x.as_f64()) {
        out["stop_loss_pct"] = json!(x);
    }
    if let Some(x) = v.get("take_profit_pct").and_then(|x| x.as_f64()) {
        out["take_profit_pct"] = json!(x);
    }
    if let Some(x) = v.get("reasoning").and_then(|x| x.as_str()) {
        out["reasoning"] = json!(x);
    }
    Ok(out)
}

fn require_action(v: &Value) -> AiResult<String> {
    let a = v
        .get("action")
        .and_then(|x| x.as_str())
        .ok_or_else(|| AiError::parse("missing action"))?;
    let a = a.trim().to_lowercase().replace(' ', "_");
    if OPERATIONAL_ACTIONS.contains(&a.as_str()) {
        Ok(a)
    } else {
        Err(AiError::parse(format!("invalid action: {a}")))
    }
}

/// Normalized operational directive JSON.
/// Strategic / portfolio JSON (looser schema; validated at DB insert).
pub fn parse_portfolio_decision(raw: &str) -> AiResult<Value> {
    let s = extract_json_block(raw)?;
    let v: Value = serde_json::from_str(&s).map_err(|e| AiError::parse(e.to_string()))?;
    Ok(v)
}

pub fn parse_operational_decision(raw: &str) -> AiResult<Value> {
    let s = extract_json_block(raw)?;
    let v: Value = serde_json::from_str(&s).map_err(|e| AiError::parse(e.to_string()))?;
    let action = require_action(&v)?;
    let mut out = json!({ "action": action });
    for key in [
        "new_stop_loss_pct",
        "new_take_profit_pct",
        "trailing_callback_pct",
        "partial_close_pct",
        "reasoning",
    ] {
        if let Some(val) = v.get(key) {
            out[key] = val.clone();
        }
    }
    if let Some(x) = v.get("open_position_ref").and_then(|x| x.as_str()) {
        out["open_position_ref"] = json!(x);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tactical_valid() {
        let raw = r#"{"direction": "buy", "confidence": 0.7, "stop_loss_pct": 1.5}"#;
        let v = parse_tactical_decision(raw).unwrap();
        assert_eq!(v["direction"], "buy");
        assert_eq!(v["confidence"], 0.7);
    }

    #[test]
    fn tactical_invalid_direction() {
        let raw = r#"{"direction": "long", "confidence": 0.5}"#;
        assert!(parse_tactical_decision(raw).is_err());
    }

    #[test]
    fn tactical_missing_field() {
        let raw = r#"{"direction": "neutral"}"#;
        assert!(parse_tactical_decision(raw).is_err());
    }

    #[test]
    fn tactical_fenced() {
        let raw = r#"Here is JSON:
```json
{"direction": "no_trade", "confidence": 0.2}
```
"#;
        let v = parse_tactical_decision(raw).unwrap();
        assert_eq!(v["direction"], "no_trade");
    }

    #[test]
    fn operational_valid() {
        let raw = r#"{"action": "tighten_stop", "new_stop_loss_pct": 0.8}"#;
        let v = parse_operational_decision(raw).unwrap();
        assert_eq!(v["action"], "tighten_stop");
    }

    #[test]
    fn operational_invalid_action() {
        let raw = r#"{"action": "panic"}"#;
        assert!(parse_operational_decision(raw).is_err());
    }

    #[test]
    fn operational_missing_action() {
        let raw = r#"{"new_stop_loss_pct": 1.0}"#;
        assert!(parse_operational_decision(raw).is_err());
    }

    #[test]
    fn tactical_confidence_out_of_range_high() {
        let raw = r#"{"direction": "buy", "confidence": 1.01}"#;
        assert!(parse_tactical_decision(raw).is_err());
    }

    #[test]
    fn tactical_confidence_out_of_range_low() {
        let raw = r#"{"direction": "buy", "confidence": -0.1}"#;
        assert!(parse_tactical_decision(raw).is_err());
    }

    #[test]
    fn tactical_multiplier_out_of_range() {
        let raw = r#"{"direction": "neutral", "confidence": 0.5, "position_size_multiplier": 2.5}"#;
        assert!(parse_tactical_decision(raw).is_err());
    }

    #[test]
    fn tactical_json_in_plain_fence() {
        let raw = r#"Output:
```
{"direction": "sell", "confidence": 0.6, "stop_loss_pct": 2.0}
```
"#;
        let v = parse_tactical_decision(raw).unwrap();
        assert_eq!(v["direction"], "sell");
    }

    #[test]
    fn operational_fenced_json() {
        let raw = r#"```json
{"action": "activate_trailing", "trailing_callback_pct": 1.0}
```"#;
        let v = parse_operational_decision(raw).unwrap();
        assert_eq!(v["action"], "activate_trailing");
    }

    #[test]
    fn tactical_gemini_fenced_unclosed_fence() {
        let raw = r#"```json
{
  "direction": "buy",
  "confidence": 0.65,
  "stop_loss_pct": 2.5,
  "reasoning": "ok"
}"#;
        let v = parse_tactical_decision(raw).unwrap();
        assert_eq!(v["direction"], "buy");
        assert_eq!(v["confidence"], 0.65);
    }

    #[test]
    fn tactical_case_insensitive_json_fence() {
        let raw = r#"```JSON
{"direction": "neutral", "confidence": 0.5}
```"#;
        let v = parse_tactical_decision(raw).unwrap();
        assert_eq!(v["direction"], "neutral");
    }

    #[test]
    fn tactical_salvage_truncated_mid_optional_key() {
        let raw = r#"{"direction": "buy", "confidence": 0.55, "stop_loss_pct": 1.5, "position_"#;
        let v = parse_tactical_decision(raw).unwrap();
        assert_eq!(v["direction"], "buy");
        assert_eq!(v["confidence"], 0.55);
        assert_eq!(v["stop_loss_pct"], 1.5);
    }

    #[test]
    fn tactical_salvage_truncated_mid_position_size_key() {
        let raw = r#"{"direction": "buy", "confidence": 0.65, "stop_loss_pct": 1.5, "position"#;
        let v = parse_tactical_decision(raw).unwrap();
        assert_eq!(v["direction"], "buy");
        assert_eq!(v["stop_loss_pct"], 1.5);
    }
}
