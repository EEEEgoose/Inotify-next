//! WeCom (企业微信) application messages.

use super::{check_api_result, ensure_success, str_field, Field, FieldKind, Message, SendCtx};
use anyhow::Context;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

/// Error codes meaning the access token is invalid or expired.
const TOKEN_ERRORS: [i64; 3] = [40001, 40014, 42001];

pub fn fields() -> Vec<Field> {
    vec![
        Field::text("corp_id", "企业 ID").required(),
        Field::text("corp_secret", "应用 Secret")
            .kind(FieldKind::Secret)
            .required(),
        Field::text("agent_id", "应用 AgentId").required(),
        Field::text("to_user", "接收人")
            .default("@all")
            .help("成员 ID，多个用 | 分隔，@all 表示全部。2022 年后创建的应用需在后台配置可信 IP"),
    ]
}

/// Access tokens per (corp id, secret), shared across sends.
#[derive(Default)]
pub struct TokenCache(Mutex<HashMap<(String, String), (String, Instant)>>);

impl TokenCache {
    async fn get(&self, ctx: &SendCtx, corp_id: &str, secret: &str) -> anyhow::Result<String> {
        let key = (corp_id.to_string(), secret.to_string());
        let mut cache = self.0.lock().await;
        if let Some((token, expires)) = cache.get(&key) {
            if Instant::now() < *expires {
                return Ok(token.clone());
            }
        }
        let resp = ctx
            .net
            .client()
            .get(format!("{}/cgi-bin/gettoken", ctx.wecom_base))
            .query(&[("corpid", corp_id), ("corpsecret", secret)])
            .send()
            .await?;
        let result: Value = ensure_success(resp).await?.json().await?;
        check_api_result(&result, "errcode", "errmsg").context("get access token")?;
        let token = result["access_token"]
            .as_str()
            .context("no access_token in response")?
            .to_string();
        // Refresh a few minutes before WeCom expires it (normally after 7200s).
        let ttl = result["expires_in"].as_u64().unwrap_or(7200).saturating_sub(300);
        cache.insert(key, (token.clone(), Instant::now() + Duration::from_secs(ttl)));
        Ok(token)
    }

    async fn invalidate(&self, corp_id: &str, secret: &str) {
        self.0.lock().await.remove(&(corp_id.to_string(), secret.to_string()));
    }
}

pub async fn send(ctx: &SendCtx, config: &Value, msg: &Message) -> anyhow::Result<()> {
    let (corp_id, secret) = (str_field(config, "corp_id"), str_field(config, "corp_secret"));
    let agent_id = str_field(config, "agent_id");
    let agent_id: Value = agent_id
        .parse::<i64>()
        .map(Value::from)
        .unwrap_or_else(|_| agent_id.into());
    let to_user = match str_field(config, "to_user") {
        "" => "@all",
        to => to,
    };
    let body = json!({
        "touser": to_user,
        "msgtype": "text",
        "agentid": agent_id,
        "text": { "content": msg.text() },
    });

    for attempt in 0..2 {
        let token = ctx.wecom_tokens.get(ctx, corp_id, secret).await?;
        let resp = ctx
            .net
            .client()
            .post(format!("{}/cgi-bin/message/send", ctx.wecom_base))
            .query(&[("access_token", token)])
            .json(&body)
            .send()
            .await?;
        let result: Value = ensure_success(resp).await?.json().await?;
        let code = result["errcode"].as_i64().unwrap_or(-1);
        if attempt == 0 && TOKEN_ERRORS.contains(&code) {
            ctx.wecom_tokens.invalidate(corp_id, secret).await;
            continue;
        }
        return check_api_result(&result, "errcode", "errmsg");
    }
    unreachable!("the second attempt always returns")
}
