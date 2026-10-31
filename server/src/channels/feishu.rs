use super::{check_api_result, str_field, Field, FieldKind, Message, SendCtx};
use anyhow::Context;
use base64::{engine::general_purpose::STANDARD, Engine};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use url::Url;

pub fn fields() -> Vec<Field> {
    vec![
        Field::text("webhook", "Webhook 地址")
            .required()
            .help("https://open.feishu.cn/open-apis/bot/v2/hook/..."),
        Field::text("secret", "签名校验密钥")
            .kind(FieldKind::Secret)
            .help("安全设置开启「签名校验」时填写"),
    ]
}

/// Feishu signature: base64(HMAC-SHA256(key = "{timestamp}\n{secret}", empty message)).
pub(crate) fn sign(timestamp: i64, secret: &str) -> String {
    let mac =
        Hmac::<Sha256>::new_from_slice(format!("{timestamp}\n{secret}").as_bytes()).expect("HMAC takes any key length");
    STANDARD.encode(mac.finalize().into_bytes())
}

pub async fn send(ctx: &SendCtx, config: &Value, msg: &Message) -> anyhow::Result<()> {
    let url = Url::parse(str_field(config, "webhook")).context("invalid webhook URL")?;
    let mut body = json!({ "msg_type": "text", "content": { "text": msg.text() } });
    let secret = str_field(config, "secret");
    if !secret.is_empty() {
        let timestamp = chrono::Utc::now().timestamp();
        body["timestamp"] = timestamp.to_string().into();
        body["sign"] = sign(timestamp, secret).into();
    }

    let client = ctx.net.client_for(&url).await?;
    let result: Value = client.post(url).json(&body).send().await?.json().await?;
    // Older bot versions answer with StatusCode/StatusMessage instead of code/msg.
    if result.get("code").is_some() {
        check_api_result(&result, "code", "msg")
    } else {
        check_api_result(&result, "StatusCode", "StatusMessage")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_like_feishu_docs() {
        // Reference value computed with the algorithm from Feishu's documentation (Python sample).
        assert_eq!(
            sign(1700000000, "feishutest"),
            "haBruccblxZRGfQ+rs6Ecglg9ZfbZEqTZYjCVhgR66E="
        );
    }
}
