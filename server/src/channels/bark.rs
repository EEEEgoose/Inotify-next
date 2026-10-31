//! Bark: the official Bark iOS app registers with this server, and pushes go straight to APNs,
//! built the same way bark-server builds them (github.com/Finb/bark-server, apns/apns.go).

use super::{str_field, Field, Message, SendCtx};
use anyhow::{bail, Context};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Bark's APNs auth key. Published by the Bark author for self-hosted servers
/// (https://github.com/Finb/Bark/blob/master/docs/en-us/cert.md); only this key can push to the
/// official app.
const AUTH_KEY: &[u8] = include_bytes!("../../assets/bark_apns_key.p8");
const KEY_ID: &str = "LH4T9V5U4R";
const TEAM_ID: &str = "5U8LBRXG3A";
const TOPIC: &str = "me.fin.bark";
const DEFAULT_SOUND: &str = "1107";
/// APNs accepts provider tokens for an hour and rejects refreshing more than every 20 minutes.
const TOKEN_TTL: Duration = Duration::from_secs(50 * 60);

pub fn fields() -> Vec<Field> {
    vec![
        Field::text("device_token", "Device Token")
            .readonly()
            .help("由 Bark App 注册时自动填写"),
        Field::text("sound", "默认铃声").help("留空使用 Bark 默认铃声"),
        Field::text("group", "默认分组"),
        Field::text("level", "默认中断级别").options(&["", "active", "timeSensitive", "passive", "critical"]),
        Field::text("is_archive", "保存到历史")
            .options(&["", "1", "0"])
            .help("留空则跟随 App 设置"),
    ]
}

pub struct Apns {
    base: String,
    key: EncodingKey,
    token: Mutex<Option<(String, Instant)>>,
}

#[derive(Serialize)]
struct ProviderClaims {
    iss: &'static str,
    iat: i64,
}

impl Apns {
    pub fn new(base: String) -> anyhow::Result<Self> {
        Ok(Self {
            base,
            key: EncodingKey::from_ec_pem(AUTH_KEY).context("load Bark APNs key")?,
            token: Mutex::new(None),
        })
    }

    async fn provider_token(&self) -> anyhow::Result<String> {
        let mut cached = self.token.lock().await;
        if let Some((token, at)) = cached.as_ref() {
            if at.elapsed() < TOKEN_TTL {
                return Ok(token.clone());
            }
        }
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(KEY_ID.into());
        let claims = ProviderClaims {
            iss: TEAM_ID,
            iat: chrono::Utc::now().timestamp(),
        };
        let token = jsonwebtoken::encode(&header, &claims, &self.key)?;
        *cached = Some((token.clone(), Instant::now()));
        Ok(token)
    }
}

/// The APNs request for one message: JSON payload, push type and collapse id.
pub(crate) fn build_payload(config: &Value, msg: &Message) -> (Value, bool, Option<String>) {
    // Channel defaults first, then the message's own parameters; keys are lowercased like bark-server.
    let mut ext: Map<String, Value> = Map::new();
    for (key, param) in [
        ("sound", "sound"),
        ("group", "group"),
        ("level", "level"),
        ("is_archive", "isarchive"),
    ] {
        let value = str_field(config, key);
        if !value.is_empty() {
            ext.insert(param.into(), value.into());
        }
    }
    for (k, v) in &msg.extras {
        ext.insert(k.to_lowercase(), v.clone());
    }

    let sound = match ext.remove("sound").as_ref().map(value_string).filter(|s| !s.is_empty()) {
        Some(s) if s.ends_with(".caf") => s,
        Some(s) => format!("{s}.caf"),
        None => format!("{DEFAULT_SOUND}.caf"),
    };
    let id = ext.get("id").map(value_string).filter(|s| !s.is_empty());
    let is_delete = ext
        .get("delete")
        .map(value_string)
        .is_some_and(|v| v == "1" || v == "1.0");

    let mut aps = Map::new();
    aps.insert("mutable-content".into(), 1.into());
    if is_delete {
        aps.insert("content-available".into(), 1.into());
    } else {
        let text = |v: &Option<String>| v.clone().filter(|s| !s.is_empty());
        let (title, subtitle, mut body) = (text(&msg.title), text(&msg.subtitle), text(&msg.body));
        if title.is_none() && subtitle.is_none() && body.is_none() {
            // APNs drops alerts without content (e.g. encrypted pushes that only carry ciphertext).
            body = Some("Empty Message".into());
        }
        let mut alert = Map::new();
        for (k, v) in [("title", title), ("subtitle", subtitle), ("body", body)] {
            if let Some(v) = v {
                alert.insert(k.into(), v.into());
            }
        }
        aps.insert("alert".into(), alert.into());
        aps.insert("sound".into(), sound.into());
        aps.insert("category".into(), "myNotificationCategory".into());
        if let Some(group) = ext.get("group") {
            aps.insert("thread-id".into(), value_string(group).into());
        }
    }

    let mut payload = Map::new();
    payload.insert("aps".into(), aps.into());
    for (k, v) in ext {
        payload.insert(k, value_string(&v).into());
    }
    (Value::Object(payload), is_delete, id)
}

fn value_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

pub async fn send(ctx: &SendCtx, channel_id: i64, config: &Value, msg: &Message) -> anyhow::Result<()> {
    let device_token = str_field(config, "device_token");
    if device_token.is_empty() {
        bail!("device token is empty, open the Bark app to register again");
    }
    if !valid_device_token(device_token) {
        bail!("device token is invalid, open the Bark app to register again");
    }
    let (payload, is_background, collapse_id) = build_payload(config, msg);
    let apns = &ctx.apns;
    let expiration = (chrono::Utc::now() + chrono::Duration::hours(24)).timestamp();

    let mut request = ctx
        .net
        .client()
        .post(format!("{}/3/device/{device_token}", apns.base))
        .bearer_auth(apns.provider_token().await?)
        .header("apns-topic", TOPIC)
        .header("apns-push-type", if is_background { "background" } else { "alert" })
        .header("apns-priority", if is_background { "5" } else { "10" })
        .header("apns-expiration", expiration.to_string())
        .json(&payload);
    if let Some(id) = collapse_id {
        request = request.header("apns-collapse-id", id);
    }

    let resp = request.send().await?;
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    let reason = resp
        .json::<Value>()
        .await
        .ok()
        .and_then(|v| v.get("reason").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_default();
    if status.as_u16() == 410 || (status.as_u16() == 400 && reason == "BadDeviceToken") {
        // The app was uninstalled or the token rotated; forget it until the app registers again.
        sqlx::query("UPDATE channels SET config = json_set(config, '$.device_token', ''), updated_at = datetime('now') WHERE id = ?")
            .bind(channel_id)
            .execute(&ctx.db)
            .await?;
    }
    bail!("APNs {status}: {reason}")
}

/// APNs device tokens are hex; anything else must not reach the request path.
pub fn valid_device_token(token: &str) -> bool {
    (1..=160).contains(&token.len()) && token.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Config for a freshly registered device.
pub fn initial_config(device_token: &str) -> Value {
    json!({ "device_token": device_token, "sound": "", "group": "", "level": "", "is_archive": "" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_matches_bark_server() {
        let config = json!({ "sound": "minuet", "group": "ops", "is_archive": "1" });
        let mut extras = Map::new();
        extras.insert("URL".into(), "https://example.com".into());
        extras.insert("badge".into(), 3.into());
        let msg = Message {
            title: Some("T".into()),
            body: Some("B".into()),
            extras,
            ..Default::default()
        };
        let (payload, background, id) = build_payload(&config, &msg);
        assert!(!background);
        assert_eq!(id, None);
        assert_eq!(
            payload,
            json!({
                "aps": {
                    "mutable-content": 1,
                    "alert": { "title": "T", "body": "B" },
                    "sound": "minuet.caf",
                    "category": "myNotificationCategory",
                    "thread-id": "ops"
                },
                "group": "ops",
                "isarchive": "1",
                "url": "https://example.com",
                "badge": "3"
            })
        );
    }

    #[test]
    fn empty_and_delete_pushes() {
        let (payload, _, _) = build_payload(&json!({}), &Message::default());
        assert_eq!(payload["aps"]["alert"]["body"], "Empty Message");
        assert_eq!(payload["aps"]["sound"], "1107.caf");

        let mut extras = Map::new();
        extras.insert("delete".into(), "1".into());
        extras.insert("id".into(), "abc".into());
        let (payload, background, id) = build_payload(
            &json!({}),
            &Message {
                extras,
                ..Default::default()
            },
        );
        assert!(background);
        assert_eq!(id.as_deref(), Some("abc"));
        assert_eq!(payload["aps"], json!({ "mutable-content": 1, "content-available": 1 }));
    }

    #[tokio::test]
    async fn signs_provider_token() {
        let apns = Apns::new("https://unused".into()).unwrap();
        let token = apns.provider_token().await.unwrap();
        let header = jsonwebtoken::decode_header(&token).unwrap();
        assert_eq!(header.alg, Algorithm::ES256);
        assert_eq!(header.kid.as_deref(), Some(KEY_ID));
        assert_eq!(apns.provider_token().await.unwrap(), token, "token should be cached");
    }
}
