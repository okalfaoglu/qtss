//! Ortak tipler.

use serde::{Deserialize, Serialize};

/// Desteklenen bildirim kanalı (API + ortam değişkenleri ile eşlenir).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationChannel {
    Telegram,
    Email,
    Sms,
    Whatsapp,
    X,
    Facebook,
    /// Meta Graph ile doğrudan metin gönderimi kısıtlıdır; çoğu kurulumda `QTSS_NOTIFY_INSTAGRAM_WEBHOOK_URL` ile otomasyon (Make/Zapier) hedeflenir.
    Instagram,
    Discord,
    /// Serbest JSON POST — Slack Incoming, özel entegrasyonlar.
    Webhook,
}

impl NotificationChannel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Telegram => "telegram",
            Self::Email => "email",
            Self::Sms => "sms",
            Self::Whatsapp => "whatsapp",
            Self::X => "x",
            Self::Facebook => "facebook",
            Self::Instagram => "instagram",
            Self::Discord => "discord",
            Self::Webhook => "webhook",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "telegram" | "tg" => Some(Self::Telegram),
            "email" | "mail" | "smtp" | "e-posta" => Some(Self::Email),
            "sms" | "twilio" => Some(Self::Sms),
            "whatsapp" | "wa" => Some(Self::Whatsapp),
            "x" | "twitter" => Some(Self::X),
            "facebook" | "fb" => Some(Self::Facebook),
            "instagram" | "ig" => Some(Self::Instagram),
            "discord" => Some(Self::Discord),
            "webhook" | "http" => Some(Self::Webhook),
            _ => None,
        }
    }
}

/// Tek bir gönderim isteği.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    /// Kısa başlık (e-posta konusu, bazı kanallarda ilk satır).
    pub title: String,
    /// Düz metin gövde.
    pub body: String,
    /// HTML gövde (yalnızca e-posta).
    #[serde(default)]
    pub body_html: Option<String>,
    /// Telegram Bot API `reply_markup` (ör. `inline_keyboard`); yalnızca Telegram kanalında kullanılır.
    #[serde(default)]
    pub telegram_reply_markup: Option<serde_json::Value>,
    /// When set, Telegram `sendMessage` uses this as the full `text` instead of `title` + newline + `body`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telegram_text: Option<String>,
    /// Telegram `parse_mode` (e.g. `HTML`). Only applied when sending to Telegram.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telegram_parse_mode: Option<String>,
    /// Optional PNG sent with [`sendPhoto`](https://core.telegram.org/bots/api#sendphoto) before the text message (plain caption).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telegram_photo_png: Option<Vec<u8>>,
    /// Short plain-text caption for `telegram_photo_png` (Telegram max 1024).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telegram_photo_caption_plain: Option<String>,
}

impl Notification {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            body_html: None,
            telegram_reply_markup: None,
            telegram_text: None,
            telegram_parse_mode: None,
            telegram_photo_png: None,
            telegram_photo_caption_plain: None,
        }
    }

    /// Telegram-only rich text; other channels still use [`Self::title`] and [`Self::body`].
    pub fn with_telegram_html_message(mut self, html: impl Into<String>) -> Self {
        self.telegram_text = Some(html.into());
        self.telegram_parse_mode = Some("HTML".into());
        self
    }

    pub fn with_html(mut self, html: impl Into<String>) -> Self {
        self.body_html = Some(html.into());
        self
    }

    pub fn with_telegram_reply_markup(mut self, markup: serde_json::Value) -> Self {
        self.telegram_reply_markup = Some(markup);
        self
    }

    /// Telegram-only PNG, sent before the HTML [`Self::telegram_text`] message. Use a plain caption (HTML applies only to `sendMessage`).
    pub fn with_telegram_photo_png(
        mut self,
        png_bytes: Vec<u8>,
        caption_plain: impl Into<String>,
    ) -> Self {
        self.telegram_photo_png = Some(png_bytes);
        self.telegram_photo_caption_plain = Some(caption_plain.into());
        self
    }
}

/// Kanal bazlı teslim özeti.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryReceipt {
    pub channel: NotificationChannel,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}
