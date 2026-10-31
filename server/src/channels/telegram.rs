use super::{ensure_success, str_field, Field, FieldKind, Message, SendCtx};
use serde_json::{json, Value};

pub fn fields() -> Vec<Field> {
    vec![
        Field::text("bot_token", "Bot Token")
            .kind(FieldKind::Secret)
            .required()
            .help("从 @BotFather 获取"),
        Field::text("chat_id", "Chat ID").required(),
    ]
}

pub async fn send(ctx: &SendCtx, config: &Value, msg: &Message) -> anyhow::Result<()> {
    let url = format!(
        "{}/bot{}/sendMessage",
        ctx.telegram_base,
        str_field(config, "bot_token")
    );
    let body = json!({ "chat_id": str_field(config, "chat_id"), "text": msg.text() });
    ensure_success(ctx.net.client().post(url).json(&body).send().await?).await?;
    Ok(())
}
