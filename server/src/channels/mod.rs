pub mod bark;
mod dingtalk;
mod email;
mod feishu;
pub mod telegram;
mod webhook;
pub mod wecom;

use crate::net::Net;
use serde::Serialize;
use serde_json::{Map, Value};
use sqlx::SqlitePool;

/// A message to deliver. `extras` carries Bark-style parameters (url, icon, group, ...);
/// channels that have no use for them ignore them.
#[derive(Debug, Clone, Default)]
pub struct Message {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub body: Option<String>,
    pub extras: Map<String, Value>,
}

impl Message {
    /// Title, subtitle and body joined by newlines, for channels that only take plain text.
    pub fn text(&self) -> String {
        [&self.title, &self.subtitle, &self.body]
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Shared resources a channel may use while sending.
pub struct SendCtx {
    pub db: SqlitePool,
    pub net: Net,
    pub telegram_base: String,
    pub wecom_base: String,
    pub wecom_tokens: wecom::TokenCache,
    pub apns: bark::Apns,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FieldKind {
    Text,
    Secret,
    Textarea,
    Bool,
    Select,
}

#[derive(Debug, Clone, Serialize)]
pub struct Field {
    pub name: &'static str,
    pub label: &'static str,
    pub kind: FieldKind,
    pub required: bool,
    pub default: Value,
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    pub options: &'static [&'static str],
    /// Shown but not editable (set by the server, e.g. the Bark device token).
    pub readonly: bool,
    pub help: &'static str,
}

impl Field {
    pub const fn text(name: &'static str, label: &'static str) -> Self {
        Self {
            name,
            label,
            kind: FieldKind::Text,
            required: false,
            default: Value::Null,
            options: &[],
            readonly: false,
            help: "",
        }
    }
    pub const fn kind(mut self, kind: FieldKind) -> Self {
        self.kind = kind;
        self
    }
    pub const fn required(mut self) -> Self {
        self.required = true;
        self
    }
    pub const fn readonly(mut self) -> Self {
        self.readonly = true;
        self
    }
    pub const fn options(mut self, options: &'static [&'static str]) -> Self {
        self.kind = FieldKind::Select;
        self.options = options;
        self
    }
    pub const fn help(mut self, help: &'static str) -> Self {
        self.help = help;
        self
    }
    pub fn default(mut self, value: impl Into<Value>) -> Self {
        self.default = value.into();
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Wecom,
    Dingtalk,
    Feishu,
    Telegram,
    Email,
    Webhook,
    Bark,
}

#[derive(Serialize)]
pub struct KindInfo {
    pub kind: &'static str,
    pub name: &'static str,
    pub fields: Vec<Field>,
    /// Users cannot create this kind by hand (Bark channels come from the app registering).
    pub user_creatable: bool,
}

impl Kind {
    pub const ALL: [Kind; 7] = [
        Kind::Wecom,
        Kind::Dingtalk,
        Kind::Feishu,
        Kind::Telegram,
        Kind::Email,
        Kind::Webhook,
        Kind::Bark,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Wecom => "wecom",
            Kind::Dingtalk => "dingtalk",
            Kind::Feishu => "feishu",
            Kind::Email => "email",
            Kind::Webhook => "webhook",
            Kind::Telegram => "telegram",
            Kind::Bark => "bark",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    pub fn info(self) -> KindInfo {
        let (name, fields) = match self {
            Kind::Wecom => ("企业微信应用", wecom::fields()),
            Kind::Dingtalk => ("钉钉群机器人", dingtalk::fields()),
            Kind::Feishu => ("飞书群机器人", feishu::fields()),
            Kind::Email => ("邮件 (SMTP)", email::fields()),
            Kind::Webhook => ("自定义 Webhook", webhook::fields()),
            Kind::Telegram => ("Telegram 机器人", telegram::fields()),
            Kind::Bark => ("Bark", bark::fields()),
        };
        KindInfo {
            kind: self.as_str(),
            name,
            fields,
            user_creatable: self != Kind::Bark,
        }
    }

    /// Keeps only known fields, fills defaults, and checks types and required fields.
    /// Read-only fields are taken from `previous` rather than from user input.
    pub fn normalize_config(self, input: &Value, previous: Option<&Value>) -> Result<Value, String> {
        let input = input.as_object().ok_or("config must be an object")?;
        let mut out = Map::new();
        for field in self.info().fields {
            let source = if field.readonly {
                previous.and_then(|p| p.get(field.name))
            } else {
                input.get(field.name)
            };
            let value = match source {
                None | Some(Value::Null) => field.default.clone(),
                Some(v) => v.clone(),
            };
            let value = match (field.kind, value) {
                (FieldKind::Bool, Value::Null) => Value::Bool(false),
                (FieldKind::Bool, v @ Value::Bool(_)) => v,
                (FieldKind::Bool, _) => return Err(format!("{} must be true or false", field.name)),
                (_, Value::Null) => Value::String(String::new()),
                (_, v @ Value::String(_)) => v,
                (_, _) => return Err(format!("{} must be a string", field.name)),
            };
            if let Value::String(s) = &value {
                if field.required && !field.readonly && s.trim().is_empty() {
                    return Err(format!("{} is required", field.label));
                }
                if field.kind == FieldKind::Select && !s.is_empty() && !field.options.contains(&s.as_str()) {
                    return Err(format!("{} must be one of {:?}", field.name, field.options));
                }
            }
            out.insert(field.name.to_string(), value);
        }
        Ok(Value::Object(out))
    }

    pub async fn send(self, ctx: &SendCtx, channel_id: i64, config: &Value, msg: &Message) -> anyhow::Result<()> {
        match self {
            Kind::Wecom => wecom::send(ctx, config, msg).await,
            Kind::Dingtalk => dingtalk::send(ctx, config, msg).await,
            Kind::Feishu => feishu::send(ctx, config, msg).await,
            Kind::Email => email::send(ctx, config, msg).await,
            Kind::Webhook => webhook::send(ctx, config, msg).await,
            Kind::Telegram => telegram::send(ctx, config, msg).await,
            Kind::Bark => bark::send(ctx, channel_id, config, msg).await,
        }
    }
}

pub(crate) fn str_field<'a>(config: &'a Value, name: &str) -> &'a str {
    config.get(name).and_then(Value::as_str).unwrap_or("")
}

/// Fails with the response status and (truncated) body unless the status is 2xx.
pub(crate) async fn ensure_success(resp: reqwest::Response) -> anyhow::Result<reqwest::Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp.text().await.unwrap_or_default();
    let body: String = body.chars().take(300).collect();
    anyhow::bail!("HTTP {status}: {body}")
}

/// Checks the `{"errcode": 0, "errmsg": ...}` style results of WeCom and DingTalk
/// (and `code`/`msg` of Feishu), which report failures with HTTP 200.
pub(crate) fn check_api_result(result: &Value, code_field: &str, msg_field: &str) -> anyhow::Result<()> {
    match result.get(code_field).and_then(Value::as_i64) {
        Some(0) => Ok(()),
        Some(code) => {
            let msg = result.get(msg_field).and_then(Value::as_str).unwrap_or("");
            anyhow::bail!("API error {code}: {msg}")
        }
        // Not echoing the response: the URL may be user-supplied.
        None => anyhow::bail!("unexpected response without `{code_field}`"),
    }
}
