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
            .help("https://oapi.dingtalk.com/robot/send?access_token=..."),
        Field::text("secret", "加签密钥")
            .kind(FieldKind::Secret)
            .help("安全设置选择「加签」时填写，以 SEC 开头"),
    ]
}

/// DingTalk signature: base64(HMAC-SHA256(key = secret, "{timestamp_ms}\n{secret}")).
pub(crate) fn sign(timestamp_ms: i64, secret: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC takes any key length");
    mac.update(format!("{timestamp_ms}\n{secret}").as_bytes());
    STANDARD.encode(mac.finalize().into_bytes())
}

pub async fn send(ctx: &SendCtx, config: &Value, msg: &Message) -> anyhow::Result<()> {
    let mut url = Url::parse(str_field(config, "webhook")).context("invalid webhook URL")?;
    let secret = str_field(config, "secret");
    if !secret.is_empty() {
        let timestamp = chrono::Utc::now().timestamp_millis();
        url.query_pairs_mut()
            .append_pair("timestamp", &timestamp.to_string())
            .append_pair("sign", &sign(timestamp, secret));
    }

    let title = msg.title.clone().filter(|t| !t.is_empty());
    let rest: Vec<&str> = [&msg.subtitle, &msg.body]
        .into_iter()
        .flatten()
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .collect();
    let text = match &title {
        Some(t) => format!("#### {t}\n\n{}", rest.join("\n\n")),
        None => rest.join("\n\n"),
    };
    let body = json!({
        "msgtype": "markdown",
        "markdown": { "title": title.unwrap_or_else(|| "Inotify".into()), "text": text },
    });

    let client = ctx.net.client_for(&url).await?;
    let result: Value = client.post(url).json(&body).send().await?.json().await?;
    check_api_result(&result, "errcode", "errmsg")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_like_dingtalk_docs() {
        // Reference value computed with the algorithm from DingTalk's documentation (Python sample).
        assert_eq!(
            sign(1700000000000, "SECtest"),
            "aZLLrriXgn05YbwaGR7knYsLeJADjr9NwLaNNKpxh4g="
        );
    }
}
