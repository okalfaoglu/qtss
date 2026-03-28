//! Ortam değişkenlerinden yapılandırma. Eksik anahtarlar = kanal devre dışı (`None`).

use serde::{Deserialize, Serialize};

/// Tüm kanallar için isteğe bağlı yapılandırma.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NotifyConfig {
    pub telegram: Option<TelegramConfig>,
    pub email: Option<EmailConfig>,
    pub sms: Option<TwilioSmsConfig>,
    pub whatsapp: Option<WhatsappCloudConfig>,
    pub x: Option<XConfig>,
    pub facebook: Option<FacebookPageConfig>,
    /// Instagram: çoğu kurulumda otomasyon web kancası (Make/Zapier → IG).
    pub instagram: Option<InstagramWebhookConfig>,
    pub discord: Option<DiscordWebhookConfig>,
    pub webhook: Option<GenericWebhookConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub chat_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailConfig {
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_username: String,
    pub smtp_password: String,
    pub from: String,
    pub to: String,
    pub starttls: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwilioSmsConfig {
    pub account_sid: String,
    pub auth_token: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhatsappCloudConfig {
    pub phone_number_id: String,
    pub access_token: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XConfig {
    /// OAuth 2.0 kullanıcı erişim jetonu (tweet göndermek için gerekli; uygulama-only bearer ile gönderim çoğu hesapta çalışmaz).
    pub bearer_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FacebookPageConfig {
    pub page_id: String,
    pub access_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstagramWebhookConfig {
    pub url: String,
    #[serde(default)]
    pub headers_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordWebhookConfig {
    pub webhook_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenericWebhookConfig {
    pub url: String,
    #[serde(default)]
    pub headers_json: Option<String>,
}

impl NotifyConfig {
    /// Ortam değişkenlerinden okur; tanımlı olmayan kanallar `None` kalır.
    pub fn from_env() -> Self {
        Self {
            telegram: read_telegram(),
            email: read_email(),
            sms: read_twilio(),
            whatsapp: read_whatsapp(),
            x: read_x(),
            facebook: read_facebook(),
            instagram: read_instagram_webhook(),
            discord: read_discord(),
            webhook: read_generic_webhook(),
        }
    }
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.trim().is_empty())
}

fn read_telegram() -> Option<TelegramConfig> {
    let bot_token = env("QTSS_NOTIFY_TELEGRAM_BOT_TOKEN")?;
    let chat_id = env("QTSS_NOTIFY_TELEGRAM_CHAT_ID")?;
    Some(TelegramConfig { bot_token, chat_id })
}

fn read_email() -> Option<EmailConfig> {
    Some(EmailConfig {
        smtp_host: env("QTSS_NOTIFY_SMTP_HOST")?,
        smtp_port: env("QTSS_NOTIFY_SMTP_PORT")
            .and_then(|s| s.parse().ok())
            .unwrap_or(587),
        smtp_username: env("QTSS_NOTIFY_SMTP_USERNAME")?,
        smtp_password: env("QTSS_NOTIFY_SMTP_PASSWORD")?,
        from: env("QTSS_NOTIFY_EMAIL_FROM")?,
        to: env("QTSS_NOTIFY_EMAIL_TO")?,
        starttls: env("QTSS_NOTIFY_SMTP_STARTTLS")
            .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
            .unwrap_or(true),
    })
}

fn read_twilio() -> Option<TwilioSmsConfig> {
    Some(TwilioSmsConfig {
        account_sid: env("QTSS_NOTIFY_TWILIO_ACCOUNT_SID")?,
        auth_token: env("QTSS_NOTIFY_TWILIO_AUTH_TOKEN")?,
        from: env("QTSS_NOTIFY_TWILIO_FROM")?,
        to: env("QTSS_NOTIFY_SMS_TO")?,
    })
}

fn read_whatsapp() -> Option<WhatsappCloudConfig> {
    Some(WhatsappCloudConfig {
        phone_number_id: env("QTSS_NOTIFY_WHATSAPP_PHONE_NUMBER_ID")?,
        access_token: env("QTSS_NOTIFY_WHATSAPP_ACCESS_TOKEN")?,
        to: env("QTSS_NOTIFY_WHATSAPP_TO")?,
    })
}

fn read_x() -> Option<XConfig> {
    Some(XConfig {
        bearer_token: env("QTSS_NOTIFY_X_BEARER_TOKEN")?,
    })
}

fn read_facebook() -> Option<FacebookPageConfig> {
    Some(FacebookPageConfig {
        page_id: env("QTSS_NOTIFY_FACEBOOK_PAGE_ID")?,
        access_token: env("QTSS_NOTIFY_FACEBOOK_PAGE_ACCESS_TOKEN")?,
    })
}

fn read_instagram_webhook() -> Option<InstagramWebhookConfig> {
    Some(InstagramWebhookConfig {
        url: env("QTSS_NOTIFY_INSTAGRAM_WEBHOOK_URL")?,
        headers_json: env("QTSS_NOTIFY_INSTAGRAM_WEBHOOK_HEADERS_JSON"),
    })
}

fn read_discord() -> Option<DiscordWebhookConfig> {
    Some(DiscordWebhookConfig {
        webhook_url: env("QTSS_NOTIFY_DISCORD_WEBHOOK_URL")?,
    })
}

fn read_generic_webhook() -> Option<GenericWebhookConfig> {
    Some(GenericWebhookConfig {
        url: env("QTSS_NOTIFY_WEBHOOK_URL")?,
        headers_json: env("QTSS_NOTIFY_WEBHOOK_HEADERS_JSON"),
    })
}
