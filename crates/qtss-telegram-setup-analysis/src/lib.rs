//! Telegram **setup analysis** webhook: collect photos/text in a per-chat buffer; run multimodal
//! LLM when the user sends a configured **trigger phrase** (default `QTSS_ANALIZ`).
//! User-facing Telegram replies are **Turkish**; operator logs stay English.

mod config;
mod gemini;
mod telegram_api;

pub use config::ResolvedSetupAnalysisConfig;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tracing::{info, warn};

/// One queued item from the user (text and/or images from a single update).
#[derive(Debug, Clone, Default)]
pub struct BufferedTurn {
    pub text_parts: Vec<String>,
    pub images: Vec<ImagePart>,
}

#[derive(Debug, Clone)]
pub struct ImagePart {
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
struct ChatBuffer {
    turns: Vec<BufferedTurn>,
    updated_unix: i64,
}

impl ChatBuffer {
    fn new_fresh() -> Self {
        Self {
            turns: Vec::new(),
            updated_unix: chrono::Utc::now().timestamp(),
        }
    }
}

/// In-memory per-`chat_id` queue. Single API instance assumed; multi-instance would need Redis/DB.
#[derive(Clone)]
pub struct SharedSetupBuffers {
    inner: Arc<Mutex<HashMap<i64, ChatBuffer>>>,
}

impl SharedSetupBuffers {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn prune_stale(map: &mut HashMap<i64, ChatBuffer>, ttl_secs: i64, now: i64) {
        map.retain(|_, v| now - v.updated_unix <= ttl_secs);
    }
}

fn update_id_of(update: &Value) -> Option<i64> {
    update.get("update_id").and_then(|x| x.as_i64())
}

fn incoming_kind(update: &Value) -> &'static str {
    if update.get("channel_post").filter(|m| !m.is_null()).is_some() {
        "channel_post"
    } else if update.get("message").filter(|m| !m.is_null()).is_some() {
        "message"
    } else {
        "none"
    }
}

fn summarize_msg(msg: &Value) -> String {
    let has_photo = msg.get("photo").and_then(|p| p.as_array()).map(|a| !a.is_empty()).unwrap_or(false);
    let has_doc = msg.get("document").is_some();
    let text_n = msg.get("text").and_then(|t| t.as_str()).map(|s| s.chars().count()).unwrap_or(0);
    let cap_n = msg
        .get("caption")
        .and_then(|t| t.as_str())
        .map(|s| s.chars().count())
        .unwrap_or(0);
    format!("has_photo={has_photo} has_document={has_doc} text_chars={text_n} caption_chars={cap_n}")
}

/// Handle a Telegram `Update` JSON. Sends replies to the same chat via Bot API when needed.
///
/// Private / group chats use `message`. **Channels** broadcast posts as `channel_post` (same shape);
/// without handling it, channel uploads are ignored and the queue never fills.
pub async fn process_telegram_update(
    http: &reqwest::Client,
    buffers: &SharedSetupBuffers,
    update: &Value,
    cfg: &ResolvedSetupAnalysisConfig,
    bot_token: &str,
) {
    let uid = update_id_of(update);
    let kind = incoming_kind(update);

    let msg = update
        .get("message")
        .filter(|m| !m.is_null())
        .or_else(|| update.get("channel_post").filter(|m| !m.is_null()));
    let Some(msg) = msg else {
        let keys: Vec<String> = update
            .as_object()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        warn!(
            target: "qtss_telegram_setup_analysis",
            update_id = ?uid,
            ?keys,
            has_edited_message = update.get("edited_message").filter(|v| !v.is_null()).is_some(),
            has_edited_channel_post = update.get("edited_channel_post").filter(|v| !v.is_null()).is_some(),
            "setup_analysis: update has no message or channel_post (edited_* not handled; nothing to do)"
        );
        return;
    };

    let chat_id = msg["chat"]["id"].as_i64();
    let Some(chat_id) = chat_id else {
        warn!(
            target: "qtss_telegram_setup_analysis",
            update_id = ?uid,
            kind,
            "setup_analysis: message/channel_post missing chat.id"
        );
        return;
    };

    if !cfg.chat_allowed(chat_id) {
        info!(
            target: "qtss_telegram_setup_analysis",
            chat_id,
            update_id = ?uid,
            allowlist_size = cfg.allowlist_size(),
            "setup_analysis: chat_id rejected by allowlist"
        );
        return;
    }

    let text = message_text_or_caption(msg);
    let is_trigger = cfg.is_trigger_message(text.as_deref());

    info!(
        target: "qtss_telegram_setup_analysis",
        update_id = ?uid,
        kind,
        chat_id,
        is_trigger,
        msg_summary = %summarize_msg(msg),
        text_preview = %text
            .as_deref()
            .map(|s| s.chars().take(80).collect::<String>())
            .unwrap_or_default(),
        "setup_analysis: parsed incoming post"
    );

    if is_trigger {
        let mut turns = {
            let mut map = buffers.inner.lock().unwrap();
            SharedSetupBuffers::prune_stale(&mut map, cfg.buffer_ttl_secs, chrono::Utc::now().timestamp());
            map.remove(&chat_id).map(|b| b.turns).unwrap_or_default()
        };

        let mut current = BufferedTurn::default();
        if let Err(e) = collect_images_from_message(http, bot_token, msg, &mut current).await {
            warn!(error = %e, chat_id, "setup_analysis trigger message media download failed");
        }
        if let Some(ref t) = text {
            let trimmed = t.trim();
            let exact = trimmed == cfg.trigger_phrase.trim();
            if !exact {
                if let Some(rest) = cfg.strip_trigger_prefix(Some(trimmed)) {
                    if !rest.trim().is_empty() {
                        current.text_parts.push(rest.trim().to_string());
                    }
                } else if !cfg.is_trigger_message(Some(trimmed)) && !trimmed.is_empty() {
                    current.text_parts.push(trimmed.to_string());
                }
            }
        }
        if !current.text_parts.is_empty() || !current.images.is_empty() {
            turns.push(current);
        }

        if turns.is_empty() {
            info!(
                target: "qtss_telegram_setup_analysis",
                update_id = ?uid,
                chat_id,
                "setup_analysis: trigger received but queue empty for this chat"
            );
            if let Err(e) = telegram_api::send_message_utf8(
                http,
                bot_token,
                chat_id,
                "Henüz kuyrukta içerik yok. Önce grafik görüntüsü veya açıklama metni gönderin; ardından analiz için tetik ifadesini yazın.",
            )
            .await
            {
                warn!(
                    target: "qtss_telegram_setup_analysis",
                    error = %e,
                    chat_id,
                    "setup_analysis: sendMessage failed (empty queue notice)"
                );
            }
            return;
        }

        let user_note = cfg.strip_trigger_prefix(text.as_deref()).map(str::trim).filter(|s| !s.is_empty());

        let turn_stats: Vec<String> = turns
            .iter()
            .enumerate()
            .map(|(i, t)| {
                format!(
                    "#{}:text_parts={} images={}",
                    i + 1,
                    t.text_parts.len(),
                    t.images.len()
                )
            })
            .collect();
        info!(
            target: "qtss_telegram_setup_analysis",
            update_id = ?uid,
            chat_id,
            turns = turns.len(),
            ?turn_stats,
            "setup_analysis: calling Gemini for merged queue"
        );

        match gemini::analyze_setup_tr_report(http, cfg, &turns, user_note).await {
            Ok(report) => {
                let chunks = split_telegram_utf8(&report, 4000);
                info!(
                    target: "qtss_telegram_setup_analysis",
                    update_id = ?uid,
                    chat_id,
                    report_chars = report.chars().count(),
                    chunk_count = chunks.len(),
                    "setup_analysis: Gemini ok, sending Telegram chunks"
                );
                for chunk in chunks {
                    if let Err(e) = telegram_api::send_message_utf8(http, bot_token, chat_id, &chunk).await {
                        warn!(
                            target: "qtss_telegram_setup_analysis",
                            error = %e,
                            chat_id,
                            chunk_chars = chunk.chars().count(),
                            "setup_analysis: sendMessage failed for report chunk"
                        );
                    }
                }
            }
            Err(e) => {
                warn!(
                    target: "qtss_telegram_setup_analysis",
                    error = %e,
                    chat_id,
                    "setup_analysis: Gemini request failed"
                );
                if let Err(se) = telegram_api::send_message_utf8(
                    http,
                    bot_token,
                    chat_id,
                    &format!(
                        "Analiz çalıştırılamadı: {}. Yapılandırmayı (Gemini API anahtarı, model) kontrol edin.",
                        e
                    ),
                )
                .await
                {
                    warn!(
                        target: "qtss_telegram_setup_analysis",
                        error = %se,
                        chat_id,
                        "setup_analysis: sendMessage failed (Gemini error notice)"
                    );
                }
            }
        }
        return;
    }

    let mut turn = BufferedTurn::default();
    if let Some(t) = text {
        let t = t.trim();
        if !t.is_empty() {
            turn.text_parts.push(t.to_string());
        }
    }

    if let Err(e) = collect_images_from_message(http, bot_token, msg, &mut turn).await {
        warn!(error = %e, chat_id, "setup_analysis download media failed");
        if let Err(se) = telegram_api::send_message_utf8(
            http,
            bot_token,
            chat_id,
            "Medya indirilemedi. Lütfen görüntüyü tekrar gönderin veya daha küçük dosya deneyin.",
        )
        .await
        {
            warn!(
                target: "qtss_telegram_setup_analysis",
                error = %se,
                chat_id,
                "setup_analysis: sendMessage failed (media error notice)"
            );
        }
        return;
    }

    if turn.text_parts.is_empty() && turn.images.is_empty() {
        info!(
            target: "qtss_telegram_setup_analysis",
            update_id = ?uid,
            chat_id,
            kind,
            msg_summary = %summarize_msg(msg),
            "setup_analysis: nothing to enqueue (no caption/text and no photo/image document)"
        );
        return;
    }

    let new_text_parts = turn.text_parts.len();
    let new_images = turn.images.len();

    let n = {
        let mut map = buffers.inner.lock().unwrap();
        SharedSetupBuffers::prune_stale(&mut map, cfg.buffer_ttl_secs, chrono::Utc::now().timestamp());
        let entry = map.entry(chat_id).or_insert_with(ChatBuffer::new_fresh);
        entry.turns.push(turn);
        if entry.turns.len() > cfg.max_buffer_turns as usize {
            let overflow = entry.turns.len() - cfg.max_buffer_turns as usize;
            entry.turns.drain(0..overflow);
        }
        entry.updated_unix = chrono::Utc::now().timestamp();
        entry.turns.len()
    };

    info!(
        target: "qtss_telegram_setup_analysis",
        update_id = ?uid,
        chat_id,
        queue_depth = n,
        new_text_parts,
        new_images,
        "setup_analysis: enqueued turn, sending ack to chat"
    );

    if let Err(e) = telegram_api::send_message_utf8(
        http,
        bot_token,
        chat_id,
        &format!(
            "✓ Kuyruğa eklendi ({} parça). Analizi başlatmak için tetik ifadesini gönderin: {}",
            n, cfg.trigger_phrase
        ),
    )
    .await
    {
        warn!(
            target: "qtss_telegram_setup_analysis",
            error = %e,
            chat_id,
            "setup_analysis: sendMessage failed (queue ack)"
        );
    }
}

async fn collect_images_from_message(
    http: &reqwest::Client,
    bot_token: &str,
    msg: &Value,
    turn: &mut BufferedTurn,
) -> Result<(), String> {
    if let Some(photos) = msg.get("photo").and_then(|p| p.as_array()) {
        if let Some(best) = photos.last() {
            if let Some(fid) = best.get("file_id").and_then(|x| x.as_str()) {
                let path = telegram_api::get_file_path(http, bot_token, fid).await?;
                let bytes = telegram_api::download_file(http, bot_token, &path).await?;
                turn.images.push(ImagePart {
                    mime_type: "image/jpeg".into(),
                    bytes,
                });
            }
        }
        return Ok(());
    }

    if let Some(doc) = msg.get("document") {
        let mime = doc
            .get("mime_type")
            .and_then(|x| x.as_str())
            .unwrap_or("application/octet-stream");
        if mime.starts_with("image/") {
            if let Some(fid) = doc.get("file_id").and_then(|x| x.as_str()) {
                let path = telegram_api::get_file_path(http, bot_token, fid).await?;
                let bytes = telegram_api::download_file(http, bot_token, &path).await?;
                turn.images.push(ImagePart {
                    mime_type: mime.to_string(),
                    bytes,
                });
            }
        }
    }

    Ok(())
}

fn message_text_or_caption(msg: &Value) -> Option<String> {
    if let Some(t) = msg.get("text").and_then(|x| x.as_str()) {
        return Some(t.to_string());
    }
    msg.get("caption")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
}

/// Split on char boundaries for Telegram's ~4096 limit; keeps chunks ≤ `max_chars`.
fn split_telegram_utf8(s: &str, max_chars: usize) -> Vec<String> {
    if s.chars().count() <= max_chars {
        return vec![s.to_string()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut count = 0usize;
    for ch in s.chars() {
        if count + 1 > max_chars {
            out.push(std::mem::take(&mut cur));
            count = 0;
        }
        cur.push(ch);
        count += 1;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    let n = out.len();
    for (i, part) in out.iter_mut().enumerate() {
        if n > 1 {
            part.insert_str(0, &format!("({}/{}) ", i + 1, n));
        }
    }
    out
}
