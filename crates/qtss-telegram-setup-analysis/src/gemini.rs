//! Gemini REST `generateContent` — multimodal setup report, Turkish output.

use base64::Engine;
use serde_json::{json, Value};

use crate::config::ResolvedSetupAnalysisConfig;
use crate::BufferedTurn;

const PROMPT_SYSTEM_EN: &str = r#"You are a disciplined trading analyst.

Internal reasoning: think step-by-step in English (structure, levels, invalidation, context).
Final answer: write EVERYTHING for the user in Turkish only. No English in the final text.

Structure the Turkish report with clear headings (markdown ## is fine):

## Varlık ve yön
- Enstrüman / sembol (tahmin edilebiliyorsa)
- Long veya Short (veya bekle / nötr)

## Seviyeler
- Giriş (Entry): tek veya bant
- Stop Loss (SL)
- Take Profit (TP): bir veya birden fazla hedef, mümkünse R çoklu

## Kurulum özeti
- 2–5 cümle: yapı, zaman dilimi, teyit, geçersizleşme

## Risk analizi
- Özet risk gerekçesi (Türkçe)
- Risk skoru: 1–10 (10 en riskli) ve kısa etiket: Düşük / Orta / Yüksek
- ASCII risk çubuğu: tam 10 karakter, dolu=kare `█`, boş=`░` örn. `[████░░░░░░] Orta (4/10)`

## Uyarı
- Yatırım tavsiyesi değildir; eğitim amaçlıdır.

If images are unclear, state assumptions explicitly in Turkish."#;

pub async fn analyze_setup_tr_report(
    client: &reqwest::Client,
    cfg: &ResolvedSetupAnalysisConfig,
    turns: &[BufferedTurn],
    user_note: Option<&str>,
) -> Result<String, String> {
    if !cfg.gemini_configured() {
        return Err("Gemini API key missing".into());
    }

    let mut parts: Vec<Value> = vec![json!({"text": PROMPT_SYSTEM_EN})];

    let mut ctx = String::from(
        "The user sent one or more messages (charts and/or notes). Combine them into one coherent setup.\n",
    );
    if let Some(n) = user_note {
        ctx.push_str("Extra instruction from the user on the trigger line:\n");
        ctx.push_str(n);
        ctx.push('\n');
    }
    parts.push(json!({"text": ctx}));

    for (i, turn) in turns.iter().enumerate() {
        for (j, t) in turn.text_parts.iter().enumerate() {
            parts.push(json!({
                "text": format!("--- Parça {} metin {} ---\n{}", i + 1, j + 1, t)
            }));
        }
        for (j, img) in turn.images.iter().enumerate() {
            let b64 = base64::engine::general_purpose::STANDARD.encode(&img.bytes);
            parts.push(json!({
                "text": format!("--- Parça {} görüntü {} ---", i + 1, j + 1)
            }));
            parts.push(json!({
                "inline_data": {
                    "mime_type": img.mime_type,
                    "data": b64
                }
            }));
        }
    }

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        urlencoding::encode(cfg.gemini_model.trim()),
        urlencoding::encode(cfg.gemini_api_key.trim())
    );

    let body = json!({
        "contents": [{
            "role": "user",
            "parts": parts
        }],
        "generationConfig": {
            "temperature": 0.35,
            "maxOutputTokens": 8192
        }
    });

    let res = client
        .post(&url)
        .json(&body)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let status = res.status();
    let txt = res.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!(
            "Gemini HTTP {}: {}",
            status,
            txt.chars().take(400).collect::<String>()
        ));
    }

    let v: Value = serde_json::from_str(&txt).map_err(|e| format!("gemini json: {e}"))?;

    if let Some(err) = v.get("error") {
        return Err(format!(
            "Gemini error: {}",
            err.to_string().chars().take(500).collect::<String>()
        ));
    }

    let parts_arr = v["candidates"]
        .get(0)
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array())
        .ok_or_else(|| "Gemini missing candidates[0].content.parts".to_string())?;

    let mut text = String::new();
    for p in parts_arr {
        if let Some(t) = p.get("text").and_then(|x| x.as_str()) {
            text.push_str(t);
        }
    }
    if text.trim().is_empty() {
        return Err("Gemini empty response".into());
    }

    Ok(text)
}
