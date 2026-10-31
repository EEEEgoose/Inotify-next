use super::{ensure_success, str_field, Field, FieldKind, Message, SendCtx};
use anyhow::Context;
use serde_json::Value;
use url::Url;

const PLACEHOLDER_HELP: &str = "可用占位符：{title} {subtitle} {body}";

pub fn fields() -> Vec<Field> {
    vec![
        Field::text("method", "请求方法")
            .options(&["POST", "GET"])
            .default("POST"),
        Field::text("url", "请求地址").required().help(PLACEHOLDER_HELP),
        Field::text("content_type", "请求体格式")
            .options(&["json", "form", "text"])
            .default("json")
            .help("仅 POST 有效"),
        Field::text("body", "请求体模板")
            .kind(FieldKind::Textarea)
            .default(r#"{"title": "{title}", "body": "{body}"}"#)
            .help("json 格式下占位符会按 JSON 字符串转义；form 格式下按 URL 编码"),
    ]
}

fn render(template: &str, msg: &Message, escape: impl Fn(&str) -> String) -> String {
    let get = |v: &Option<String>| escape(v.as_deref().unwrap_or(""));
    template
        .replace("{title}", &get(&msg.title))
        .replace("{subtitle}", &get(&msg.subtitle))
        .replace("{body}", &get(&msg.body))
}

fn url_encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

fn json_escape(s: &str) -> String {
    let quoted = Value::String(s.to_string()).to_string();
    quoted[1..quoted.len() - 1].to_string()
}

pub async fn send(ctx: &SendCtx, config: &Value, msg: &Message) -> anyhow::Result<()> {
    let url = render(str_field(config, "url"), msg, url_encode);
    let url = Url::parse(&url).context("invalid webhook URL")?;
    let client = ctx.net.client_for(&url).await?;

    let request = if str_field(config, "method") == "GET" {
        client.get(url)
    } else {
        let template = str_field(config, "body");
        let (content_type, body) = match str_field(config, "content_type") {
            "form" => ("application/x-www-form-urlencoded", render(template, msg, url_encode)),
            "text" => ("text/plain; charset=utf-8", render(template, msg, str::to_string)),
            _ => ("application/json", render(template, msg, json_escape)),
        };
        client
            .post(url)
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(body)
    };
    ensure_success(request.send().await?).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_per_format() {
        let msg = Message {
            title: Some("a\"b".into()),
            body: Some("x&y z".into()),
            ..Default::default()
        };
        assert_eq!(render(r#"{"t":"{title}"}"#, &msg, json_escape), r#"{"t":"a\"b"}"#);
        assert_eq!(render("b={body}", &msg, url_encode), "b=x%26y+z");
    }
}
