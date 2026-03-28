//! Çok kanallı gönderim.

use crate::config::NotifyConfig;
use crate::error::{NotifyError, NotifyResult};
use crate::types::{DeliveryReceipt, Notification, NotificationChannel};
use base64::Engine;
use serde_json::json;

/// Tüm kanalları tek `reqwest` istemcisi ile yönetir.
#[derive(Debug, Clone)]
pub struct NotificationDispatcher {
    client: reqwest::Client,
    config: NotifyConfig,
}

impl NotificationDispatcher {
    pub fn new(config: NotifyConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(45))
            .user_agent(concat!("qtss-notify/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("reqwest client");
        Self { client, config }
    }

    pub fn from_env() -> Self {
        Self::new(NotifyConfig::from_env())
    }

    pub fn config(&self) -> &NotifyConfig {
        &self.config
    }

    /// Tek kanal gönder; hata durumunda `Err` döner.
    pub async fn send(&self, channel: NotificationChannel, n: &Notification) -> NotifyResult<DeliveryReceipt> {
        match channel {
            NotificationChannel::Telegram => self.send_telegram(n).await,
            NotificationChannel::Email => self.send_email(n).await,
            NotificationChannel::Sms => self.send_sms_twilio(n).await,
            NotificationChannel::Whatsapp => self.send_whatsapp(n).await,
            NotificationChannel::X => self.send_x(n).await,
            NotificationChannel::Facebook => self.send_facebook(n).await,
            NotificationChannel::Instagram => self.send_instagram_webhook(n).await,
            NotificationChannel::Discord => self.send_discord(n).await,
            NotificationChannel::Webhook => self.send_generic_webhook(n).await,
        }
    }

    /// Birden fazla kanal; her biri için sonuç (hata olsa bile diğerleri çalışır).
    pub async fn send_all(&self, channels: &[NotificationChannel], n: &Notification) -> Vec<DeliveryReceipt> {
        let mut v = Vec::with_capacity(channels.len());
        for c in channels {
            let r = match self.send(*c, n).await {
                Ok(rec) => rec,
                Err(e) => DeliveryReceipt {
                    channel: *c,
                    ok: false,
                    provider_id: None,
                    detail: Some(e.to_string()),
                },
            };
            v.push(r);
        }
        v
    }

    async fn send_telegram(&self, n: &Notification) -> NotifyResult<DeliveryReceipt> {
        let c = self
            .config
            .telegram
            .as_ref()
            .ok_or_else(|| NotifyError::ChannelNotConfigured("telegram".into()))?;
        let url = format!("https://api.telegram.org/bot{}/sendMessage", c.bot_token);
        let text = format!("{}\n{}", n.title, n.body);
        let body = json!({
            "chat_id": c.chat_id,
            "text": text
        });
        let res = self.client.post(&url).json(&body).send().await.map_err(|e| {
            NotifyError::Transport(e.to_string())
        })?;
        let status = res.status();
        let txt = res.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(NotifyError::Http {
                status: status.as_u16(),
                body: txt,
            });
        }
        let v: serde_json::Value = serde_json::from_str(&txt).unwrap_or(json!({}));
        let mid = v["result"]["message_id"].as_i64().map(|x| x.to_string());
        Ok(DeliveryReceipt {
            channel: NotificationChannel::Telegram,
            ok: true,
            provider_id: mid,
            detail: None,
        })
    }

    async fn send_email(&self, n: &Notification) -> NotifyResult<DeliveryReceipt> {
        use lettre::message::{header::ContentType, Mailbox, Message, MultiPart, SinglePart};
        use lettre::transport::smtp::authentication::Credentials;
        use lettre::AsyncSmtpTransport;
        use lettre::AsyncTransport;
        use lettre::Tokio1Executor;

        let c = self
            .config
            .email
            .as_ref()
            .ok_or_else(|| NotifyError::ChannelNotConfigured("email".into()))?;
        let from: Mailbox = c
            .from
            .parse()
            .map_err(|e: lettre::address::AddressError| NotifyError::Email(e.to_string()))?;
        let to: Mailbox = c
            .to
            .parse()
            .map_err(|e: lettre::address::AddressError| NotifyError::Email(e.to_string()))?;

        let email = if let Some(ref html) = n.body_html {
            Message::builder()
                .from(from)
                .to(to)
                .subject(&n.title)
                .multipart(
                    MultiPart::alternative()
                        .singlepart(
                            SinglePart::builder()
                                .header(ContentType::TEXT_PLAIN)
                                .body(n.body.clone()),
                        )
                        .singlepart(
                            SinglePart::builder()
                                .header(ContentType::TEXT_HTML)
                                .body(html.clone()),
                        ),
                )
                .map_err(|e| NotifyError::Email(e.to_string()))?
        } else {
            Message::builder()
                .from(from)
                .to(to)
                .subject(&n.title)
                .singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_PLAIN)
                        .body(n.body.clone()),
                )
                .map_err(|e| NotifyError::Email(e.to_string()))?
        };

        let creds = Credentials::new(c.smtp_username.clone(), c.smtp_password.clone());
        let mailer = {
            let b = AsyncSmtpTransport::<Tokio1Executor>::relay(&c.smtp_host)
                .map_err(|e| NotifyError::Email(e.to_string()))?
                .credentials(creds)
                .port(c.smtp_port);
            if c.starttls {
                b.tls(lettre::transport::smtp::client::Tls::Opportunistic(
                    lettre::transport::smtp::client::TlsParameters::new(c.smtp_host.clone())
                        .map_err(|e| NotifyError::Email(e.to_string()))?,
                ))
                .build()
            } else {
                b.build()
            }
        };

        mailer
            .send(email)
            .await
            .map_err(|e| NotifyError::Email(e.to_string()))?;

        Ok(DeliveryReceipt {
            channel: NotificationChannel::Email,
            ok: true,
            provider_id: None,
            detail: None,
        })
    }

    async fn send_sms_twilio(&self, n: &Notification) -> NotifyResult<DeliveryReceipt> {
        let c = self
            .config
            .sms
            .as_ref()
            .ok_or_else(|| NotifyError::ChannelNotConfigured("sms".into()))?;
        let url = format!(
            "https://api.twilio.com/2010-04-01/Accounts/{}/Messages.json",
            c.account_sid
        );
        let text = format!("{} — {}", n.title, n.body);
        let form = [
            ("To", c.to.as_str()),
            ("From", c.from.as_str()),
            ("Body", text.as_str()),
        ];
        let auth = base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", c.account_sid, c.auth_token).as_bytes());
        let res = self
            .client
            .post(&url)
            .header("Authorization", format!("Basic {}", auth))
            .form(&form)
            .send()
            .await
            .map_err(|e| NotifyError::Transport(e.to_string()))?;
        let status = res.status();
        let txt = res.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(NotifyError::Http {
                status: status.as_u16(),
                body: txt,
            });
        }
        let v: serde_json::Value = serde_json::from_str(&txt).unwrap_or(json!({}));
        let sid = v["sid"].as_str().map(String::from);
        Ok(DeliveryReceipt {
            channel: NotificationChannel::Sms,
            ok: true,
            provider_id: sid,
            detail: None,
        })
    }

    async fn send_whatsapp(&self, n: &Notification) -> NotifyResult<DeliveryReceipt> {
        let c = self
            .config
            .whatsapp
            .as_ref()
            .ok_or_else(|| NotifyError::ChannelNotConfigured("whatsapp".into()))?;
        let url = format!(
            "https://graph.facebook.com/v21.0/{}/messages",
            c.phone_number_id
        );
        let body = json!({
            "messaging_product": "whatsapp",
            "to": c.to,
            "type": "text",
            "text": { "body": format!("*{}*\n{}", n.title, n.body) }
        });
        let res = self
            .client
            .post(&url)
            .bearer_auth(&c.access_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| NotifyError::Transport(e.to_string()))?;
        let status = res.status();
        let txt = res.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(NotifyError::Http {
                status: status.as_u16(),
                body: txt,
            });
        }
        let v: serde_json::Value = serde_json::from_str(&txt).unwrap_or(json!({}));
        let mid = v["messages"][0]["id"].as_str().map(String::from);
        Ok(DeliveryReceipt {
            channel: NotificationChannel::Whatsapp,
            ok: true,
            provider_id: mid,
            detail: None,
        })
    }

    async fn send_x(&self, n: &Notification) -> NotifyResult<DeliveryReceipt> {
        let c = self
            .config
            .x
            .as_ref()
            .ok_or_else(|| NotifyError::ChannelNotConfigured("x".into()))?;
        let text = truncate_chars(&format!("{} — {}", n.title, n.body), 280);
        let body = json!({ "text": text });
        let res = self
            .client
            .post("https://api.twitter.com/2/tweets")
            .bearer_auth(&c.bearer_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| NotifyError::Transport(e.to_string()))?;
        let status = res.status();
        let txt = res.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(NotifyError::Http {
                status: status.as_u16(),
                body: txt,
            });
        }
        let v: serde_json::Value = serde_json::from_str(&txt).unwrap_or(json!({}));
        let id = v["data"]["id"].as_str().map(String::from);
        Ok(DeliveryReceipt {
            channel: NotificationChannel::X,
            ok: true,
            provider_id: id,
            detail: None,
        })
    }

    async fn send_facebook(&self, n: &Notification) -> NotifyResult<DeliveryReceipt> {
        let c = self
            .config
            .facebook
            .as_ref()
            .ok_or_else(|| NotifyError::ChannelNotConfigured("facebook".into()))?;
        let msg = format!("{}\n\n{}", n.title, n.body);
        let url = format!(
            "https://graph.facebook.com/v21.0/{}/feed",
            c.page_id
        );
        let res = self
            .client
            .post(&url)
            .query(&[
                ("message", msg.as_str()),
                ("access_token", c.access_token.as_str()),
            ])
            .send()
            .await
            .map_err(|e| NotifyError::Transport(e.to_string()))?;
        let status = res.status();
        let txt = res.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(NotifyError::Http {
                status: status.as_u16(),
                body: txt,
            });
        }
        let v: serde_json::Value = serde_json::from_str(&txt).unwrap_or(json!({}));
        let id = v["id"].as_str().map(String::from);
        Ok(DeliveryReceipt {
            channel: NotificationChannel::Facebook,
            ok: true,
            provider_id: id,
            detail: None,
        })
    }

    async fn send_instagram_webhook(&self, n: &Notification) -> NotifyResult<DeliveryReceipt> {
        let c = self
            .config
            .instagram
            .as_ref()
            .ok_or_else(|| NotifyError::ChannelNotConfigured("instagram".into()))?;
        post_json_webhook(
            &self.client,
            &c.url,
            c.headers_json.as_deref(),
            &json!({
                "source": "qtss",
                "channel": "instagram",
                "title": n.title,
                "body": n.body,
                "body_html": n.body_html,
            }),
        )
        .await?;
        Ok(DeliveryReceipt {
            channel: NotificationChannel::Instagram,
            ok: true,
            provider_id: None,
            detail: Some("webhook".into()),
        })
    }

    async fn send_discord(&self, n: &Notification) -> NotifyResult<DeliveryReceipt> {
        let c = self
            .config
            .discord
            .as_ref()
            .ok_or_else(|| NotifyError::ChannelNotConfigured("discord".into()))?;
        let content = format!("**{}**\n{}", n.title, n.body);
        let body = json!({ "content": truncate_chars(&content, 2000) });
        let res = self
            .client
            .post(&c.webhook_url)
            .json(&body)
            .send()
            .await
            .map_err(|e| NotifyError::Transport(e.to_string()))?;
        let status = res.status();
        let txt = res.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(NotifyError::Http {
                status: status.as_u16(),
                body: txt,
            });
        }
        Ok(DeliveryReceipt {
            channel: NotificationChannel::Discord,
            ok: true,
            provider_id: None,
            detail: None,
        })
    }

    async fn send_generic_webhook(&self, n: &Notification) -> NotifyResult<DeliveryReceipt> {
        let c = self
            .config
            .webhook
            .as_ref()
            .ok_or_else(|| NotifyError::ChannelNotConfigured("webhook".into()))?;
        post_json_webhook(
            &self.client,
            &c.url,
            c.headers_json.as_deref(),
            &json!({
                "source": "qtss",
                "channel": "webhook",
                "title": n.title,
                "body": n.body,
                "body_html": n.body_html,
            }),
        )
        .await?;
        Ok(DeliveryReceipt {
            channel: NotificationChannel::Webhook,
            ok: true,
            provider_id: None,
            detail: None,
        })
    }
}

async fn post_json_webhook(
    client: &reqwest::Client,
    url: &str,
    headers_json: Option<&str>,
    payload: &serde_json::Value,
) -> NotifyResult<()> {
    let mut req = client.post(url).json(payload);
    if let Some(h) = headers_json {
        let map: std::collections::HashMap<String, String> =
            serde_json::from_str(h).map_err(|e| NotifyError::Transport(format!("WEBHOOK_HEADERS_JSON: {e}")))?;
        for (k, v) in map {
            req = req.header(k, v);
        }
    }
    let res = req.send().await.map_err(|e| NotifyError::Transport(e.to_string()))?;
    let status = res.status();
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(NotifyError::Http {
            status: status.as_u16(),
            body,
        });
    }
    Ok(())
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect()
}

