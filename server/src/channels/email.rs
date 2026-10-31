use super::{str_field, Field, FieldKind, Message, SendCtx};
use anyhow::Context;
use lettre::{
    message::{header::ContentType, Mailbox},
    transport::smtp::{
        authentication::Credentials,
        client::{Tls, TlsParameters},
    },
    AsyncSmtpTransport, AsyncTransport, Tokio1Executor,
};
use serde_json::Value;

pub fn fields() -> Vec<Field> {
    vec![
        Field::text("host", "SMTP 服务器").required().help("例如 smtp.qq.com"),
        Field::text("port", "端口").default("465"),
        Field::text("security", "加密方式")
            .options(&["tls", "starttls", "none"])
            .default("tls")
            .help("465 端口一般用 tls，587 端口用 starttls"),
        Field::text("username", "用户名").help("通常是完整邮箱地址"),
        Field::text("password", "密码 / 授权码").kind(FieldKind::Secret),
        Field::text("from", "发件人")
            .required()
            .help("例如 Inotify <me@example.com>"),
        Field::text("to", "收件人").required().help("多个用英文逗号分隔"),
    ]
}

pub(crate) fn build_email(config: &Value, msg: &Message) -> anyhow::Result<lettre::Message> {
    let from: Mailbox = str_field(config, "from").parse().context("invalid sender address")?;
    let mut builder = lettre::Message::builder().from(from).subject(
        msg.title
            .clone()
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "Inotify 通知".into()),
    );
    for to in str_field(config, "to")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        builder = builder.to(to.parse().with_context(|| format!("invalid recipient {to}"))?);
    }
    let body = [&msg.subtitle, &msg.body]
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join("\n\n");
    Ok(builder.header(ContentType::TEXT_PLAIN).body(body)?)
}

pub async fn send(ctx: &SendCtx, config: &Value, msg: &Message) -> anyhow::Result<()> {
    let email = build_email(config, msg)?;
    let host = str_field(config, "host");
    let port: u16 = match str_field(config, "port") {
        "" => 465,
        p => p.parse().context("invalid port")?,
    };
    // Connect to the checked address; TLS still verifies the certificate against `host`.
    let addr = ctx.net.resolve(host, port).await?;
    let tls = TlsParameters::new(host.to_string());
    let tls = match str_field(config, "security") {
        "none" => Tls::None,
        "starttls" => Tls::Required(tls?),
        _ => Tls::Wrapper(tls?),
    };
    let mut transport = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(addr.ip().to_string())
        .port(port)
        .tls(tls)
        .timeout(Some(crate::net::TIMEOUT));
    let username = str_field(config, "username");
    if !username.is_empty() {
        transport = transport.credentials(Credentials::new(username.into(), str_field(config, "password").into()));
    }
    transport.build().send(email).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn builds_message() {
        let config = json!({ "from": "Inotify <a@example.com>", "to": "b@example.com, c@example.com" });
        let msg = Message {
            title: Some("T".into()),
            body: Some("B".into()),
            ..Default::default()
        };
        let raw = String::from_utf8(build_email(&config, &msg).unwrap().formatted()).unwrap();
        assert!(raw.contains("Subject: T"), "{raw}");
        assert!(raw.contains("To: b@example.com, c@example.com"), "{raw}");
        assert!(build_email(&json!({ "from": "nope", "to": "b@example.com" }), &msg).is_err());
    }
}
