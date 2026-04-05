//! Minimal Telegram Bot HTTP helpers (file download + sendMessage).

use serde_json::{json, Value};

pub async fn get_file_path(client: &reqwest::Client, bot_token: &str, file_id: &str) -> Result<String, String> {
    let url = format!(
        "https://api.telegram.org/bot{}/getFile?file_id={}",
        bot_token,
        urlencoding::encode(file_id)
    );
    let res = client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = res.status();
    let txt = res.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("getFile HTTP {}: {}", status, txt.chars().take(200).collect::<String>()));
    }
    let v: Value = serde_json::from_str(&txt).map_err(|e| e.to_string())?;
    if !v["ok"].as_bool().unwrap_or(false) {
        return Err("getFile ok=false".into());
    }
    v["result"]["file_path"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "getFile missing file_path".into())
}

pub async fn download_file(client: &reqwest::Client, bot_token: &str, file_path: &str) -> Result<Vec<u8>, String> {
    let url = format!(
        "https://api.telegram.org/file/bot{}/{}",
        bot_token,
        file_path
    );
    let res = client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("download HTTP {}", res.status()));
    }
    res.bytes().await.map(|b| b.to_vec()).map_err(|e| e.to_string())
}

pub async fn send_message_utf8(
    client: &reqwest::Client,
    bot_token: &str,
    chat_id: i64,
    text: &str,
) -> Result<(), String> {
    let url = format!("https://api.telegram.org/bot{}/sendMessage", bot_token);
    let body = json!({
        "chat_id": chat_id,
        "text": text,
        "disable_web_page_preview": true,
    });
    let res = client
        .post(url)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = res.status();
    let txt = res.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("sendMessage HTTP {}: {}", status, txt.chars().take(300).collect::<String>()));
    }
    let v: Value = serde_json::from_str(&txt).unwrap_or(json!({}));
    if !v["ok"].as_bool().unwrap_or(false) {
        return Err(format!("sendMessage ok=false: {}", txt.chars().take(200).collect::<String>()));
    }
    Ok(())
}
