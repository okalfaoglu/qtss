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

/// Pulls a JSON object from markdown fences or the first `{`…`}` span.
pub fn extract_json_block(raw: &str) -> AiResult<String> {
    let t = raw.trim();
    if let Some(start) = t.find("```json") {
        let after = &t[start + "```json".len()..];
        if let Some(end) = after.find("```") {
            return Ok(after[..end].trim().to_string());
        }
    }
    if let Some(start) = t.find("```") {
        let after = &t[start + 3..];
        if let Some(line_end) = after.find('\n') {
            let rest = &after[line_end + 1..];
            if let Some(end) = rest.find("```") {
                return Ok(rest[..end].trim().to_string());
            }
        }
    }
    let start = t
        .find('{')
        .ok_or_else(|| AiError::parse("no JSON object start"))?;
    let slice = &t[start..];
    let end = find_matching_brace(slice).ok_or_else(|| AiError::parse("unbalanced JSON braces"))?;
    Ok(slice[..=end].to_string())
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
    let c = v
        .get("confidence")
        .and_then(|x| x.as_f64())
        .ok_or_else(|| AiError::parse("missing confidence"))?;
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
    let s = extract_json_block(raw)?;
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
}
