//! Channels added in step 3, GitHub login, settings and stats.

use super::{app, wait_requests, App};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};
use tower::ServiceExt;
use wiremock::{
    matchers::{method, path, query_param},
    Mock, MockServer, ResponseTemplate,
};

async fn create_and_test(app: &App, token: &str, kind: &str, config: Value) -> Value {
    let (status, channel) = app
        .call(
            Method::POST,
            "/api/v1/channels",
            Some(token),
            Some(json!({ "kind": kind, "name": kind, "config": config })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{channel}");
    let (_, result) = app
        .call(
            Method::POST,
            &format!("/api/v1/channels/{}/test", channel["id"]),
            Some(token),
            None,
        )
        .await;
    result
}

#[tokio::test]
async fn dingtalk_signs_and_checks_errcode() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/robot/send"))
        .and(query_param("access_token", "good"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "errcode": 0, "errmsg": "ok" })))
        .mount(&mock)
        .await;
    Mock::given(method("POST"))
        .and(path("/robot/send"))
        .and(query_param("access_token", "bad"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "errcode": 310000, "errmsg": "sign not match" })),
        )
        .mount(&mock)
        .await;
    let app = app(&mock).await;
    let (token, _) = app.admin().await;

    let ok = create_and_test(
        &app,
        &token,
        "dingtalk",
        json!({ "webhook": format!("{}/robot/send?access_token=good", mock.uri()), "secret": "SECx" }),
    )
    .await;
    assert_eq!(ok, json!({ "ok": true }));
    let req = &mock.received_requests().await.unwrap()[0];
    let query: Vec<(String, String)> = req.url.query_pairs().into_owned().collect();
    assert!(
        query.iter().any(|(k, _)| k == "timestamp") && query.iter().any(|(k, _)| k == "sign"),
        "{query:?}"
    );
    let body: Value = serde_json::from_slice(&req.body).unwrap();
    assert_eq!(body["msgtype"], "markdown");
    assert_eq!(body["markdown"]["title"], "Inotify 测试消息");

    let bad = create_and_test(
        &app,
        &token,
        "dingtalk",
        json!({ "webhook": format!("{}/robot/send?access_token=bad", mock.uri()) }),
    )
    .await;
    assert_eq!(bad["ok"], false);
    assert!(bad["error"].as_str().unwrap().contains("310000"), "{bad}");
}

#[tokio::test]
async fn feishu_signs_body() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/open-apis/bot/v2/hook/abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "code": 0, "msg": "success" })))
        .mount(&mock)
        .await;
    let app = app(&mock).await;
    let (token, _) = app.admin().await;
    let result = create_and_test(
        &app,
        &token,
        "feishu",
        json!({ "webhook": format!("{}/open-apis/bot/v2/hook/abc", mock.uri()), "secret": "s" }),
    )
    .await;
    assert_eq!(result, json!({ "ok": true }));
    let body: Value = serde_json::from_slice(&mock.received_requests().await.unwrap()[0].body).unwrap();
    assert_eq!(body["msg_type"], "text");
    assert_eq!(body["content"]["text"], "Inotify 测试消息\n渠道「feishu」配置正确");
    assert!(body["sign"].is_string() && body["timestamp"].is_string(), "{body}");
}

#[tokio::test]
async fn wecom_caches_token_and_retries_when_expired() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/cgi-bin/gettoken"))
        .and(query_param("corpid", "corp"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "errcode": 0, "access_token": "T", "expires_in": 7200 })),
        )
        .expect(2)
        .mount(&mock)
        .await;
    // First send succeeds; the second is told the token expired once, then succeeds.
    Mock::given(method("POST"))
        .and(path("/cgi-bin/message/send"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "errcode": 0 })))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&mock)
        .await;
    Mock::given(method("POST"))
        .and(path("/cgi-bin/message/send"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "errcode": 42001, "errmsg": "expired" })))
        .up_to_n_times(1)
        .with_priority(2)
        .mount(&mock)
        .await;
    Mock::given(method("POST"))
        .and(path("/cgi-bin/message/send"))
        .and(query_param("access_token", "T"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "errcode": 0 })))
        .with_priority(3)
        .mount(&mock)
        .await;
    let app = app(&mock).await;
    let (token, _) = app.admin().await;
    let config = json!({ "corp_id": "corp", "corp_secret": "sec", "agent_id": "1000002" });
    assert_eq!(
        create_and_test(&app, &token, "wecom", config.clone()).await,
        json!({ "ok": true })
    );
    assert_eq!(
        create_and_test(&app, &token, "wecom", config).await,
        json!({ "ok": true })
    );

    let sends: Vec<Value> = mock
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/cgi-bin/message/send")
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect();
    assert_eq!(sends.len(), 3);
    assert_eq!(sends[0]["agentid"], 1000002);
    assert_eq!(sends[0]["touser"], "@all");
    // `.expect(2)` on gettoken is verified when the mock server drops.
}

/// A minimal SMTP server that accepts one message and returns its DATA.
async fn fake_smtp() -> (u16, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read, mut write) = socket.into_split();
        let mut lines = BufReader::new(read).lines();
        write.write_all(b"220 fake ESMTP\r\n").await.unwrap();
        let mut data = String::new();
        while let Some(line) = lines.next_line().await.unwrap() {
            let command = line.to_ascii_uppercase();
            let reply: &[u8] = if command.starts_with("EHLO") || command.starts_with("HELO") {
                b"250 fake\r\n"
            } else if command.starts_with("DATA") {
                write.write_all(b"354 go ahead\r\n").await.unwrap();
                while let Some(line) = lines.next_line().await.unwrap() {
                    if line == "." {
                        break;
                    }
                    data.push_str(&line);
                    data.push('\n');
                }
                b"250 queued\r\n"
            } else if command.starts_with("QUIT") {
                write.write_all(b"221 bye\r\n").await.unwrap();
                break;
            } else {
                b"250 ok\r\n"
            };
            write.write_all(reply).await.unwrap();
        }
        data
    });
    (port, handle)
}

#[tokio::test]
async fn email_over_smtp() {
    let mock = MockServer::start().await;
    let app = app(&mock).await;
    let (token, _) = app.admin().await;
    let (port, smtp) = fake_smtp().await;
    let result = create_and_test(
        &app,
        &token,
        "email",
        json!({
            "host": "127.0.0.1",
            "port": port.to_string(),
            "security": "none",
            "from": "Inotify <bot@example.com>",
            "to": "me@example.com"
        }),
    )
    .await;
    assert_eq!(result, json!({ "ok": true }));
    let data = smtp.await.unwrap();
    assert!(data.contains("To: me@example.com"), "{data}");
    assert!(
        data.contains("Subject: Inotify =?utf-8?b?"),
        "non-ASCII subject is encoded: {data}"
    );
}

async fn raw(app: &App, req: Request<Body>) -> axum::response::Response {
    app.router.clone().oneshot(req).await.unwrap()
}

fn location(resp: &axum::response::Response) -> String {
    resp.headers()[header::LOCATION].to_str().unwrap().to_string()
}

#[tokio::test]
async fn github_login_never_takes_over_local_accounts() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login/oauth/access_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "access_token": "gho_x" })))
        .mount(&mock)
        .await;
    // A GitHub user whose login equals the local administrator's username.
    Mock::given(method("GET"))
        .and(path("/user"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "id": 99, "login": "admin", "avatar_url": "https://a/x.png" })),
        )
        .mount(&mock)
        .await;
    let app = app(&mock).await;
    let (token, _) = app.admin().await;

    let (_, options) = app.call(Method::GET, "/api/v1/auth-options", None, None).await;
    assert_eq!(options["github"], false);

    let (status, _) = app
        .call(
            Method::PUT,
            "/api/v1/settings",
            Some(&token),
            Some(json!({ "proxy": "::not a url::" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, settings) = app
        .call(
            Method::PUT,
            "/api/v1/settings",
            Some(&token),
            Some(json!({
                "github_enabled": true,
                "github_client_id": "cid",
                "github_client_secret": "csecret",
                "github_allow_signup": true,
                "public_url": "https://inotify.example.com/"
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{settings}");
    assert_eq!(settings["github_client_secret"], "");
    assert_eq!(settings["github_client_secret_set"], true);
    assert_eq!(
        settings["github_callback_url"],
        "https://inotify.example.com/api/v1/oauth/github/callback"
    );

    // Saving again without the secret keeps it.
    let mut again = settings.clone();
    again
        .as_object_mut()
        .unwrap()
        .retain(|k, _| !k.ends_with("_set") && !k.ends_with("_url") || k == "public_url");
    app.call(Method::PUT, "/api/v1/settings", Some(&token), Some(again))
        .await;
    let (_, options) = app.call(Method::GET, "/api/v1/auth-options", None, None).await;
    assert_eq!(options["github"], true);

    // Start: redirect to GitHub with a state, which is also set as a cookie.
    let resp = raw(
        &app,
        Request::get("/api/v1/oauth/github/start").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let to = url::Url::parse(&location(&resp)).unwrap();
    assert_eq!(to.path(), "/login/oauth/authorize");
    let state: String = to.query_pairs().find(|(k, _)| k == "state").unwrap().1.into();
    let cookie = resp.headers()[header::SET_COOKIE].to_str().unwrap().to_string();
    assert!(cookie.contains("HttpOnly") && cookie.contains("Secure"), "{cookie}");
    let cookie = cookie.split(';').next().unwrap().to_string();

    // Without the cookie (another browser), the callback is refused.
    let callback = format!("/api/v1/oauth/github/callback?code=c&state={state}");
    let resp = raw(&app, Request::get(&callback).body(Body::empty()).unwrap()).await;
    assert!(location(&resp).starts_with("/#/login?error="));

    let resp = raw(
        &app,
        Request::get(&callback)
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    let to = location(&resp);
    let jwt = to.strip_prefix("/#/oauth?token=").unwrap_or_else(|| panic!("{to}"));
    let (_, me) = app.call(Method::GET, "/api/v1/me", Some(jwt), None).await;
    assert_eq!(me["username"], "admin-2");
    assert_eq!(me["is_admin"], false);
    assert_eq!(me["has_password"], false);
    assert_eq!(me["github_login"], "admin");

    // The state is single-use.
    let resp = raw(
        &app,
        Request::get(&callback)
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert!(location(&resp).starts_with("/#/login?error="));

    // That GitHub account is now taken: linking it to the administrator fails.
    let (_, link) = app
        .call(Method::POST, "/api/v1/oauth/github/link", Some(&token), None)
        .await;
    let resp = raw(
        &app,
        Request::get(link["url"].as_str().unwrap()).body(Body::empty()).unwrap(),
    )
    .await;
    let to = url::Url::parse(&location(&resp)).unwrap();
    let state: String = to.query_pairs().find(|(k, _)| k == "state").unwrap().1.into();
    let cookie = resp.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let resp = raw(
        &app,
        Request::get(format!("/api/v1/oauth/github/callback?code=c&state={state}"))
            .header(header::COOKIE, cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    let to = location(&resp);
    assert!(to.starts_with("/#/login?error="), "{to}");

    // The GitHub-only account can set a first password without an old one, then unlink.
    let (status, _) = app
        .call(
            Method::PUT,
            "/api/v1/me/password",
            Some(jwt),
            Some(json!({ "new_password": "a-new-password" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, me) = app.call(Method::DELETE, "/api/v1/me/github", Some(jwt), None).await;
    assert_eq!(me["github_login"], Value::Null);
}

#[tokio::test]
async fn stats_and_static_fallback() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock)
        .await;
    let app = app(&mock).await;
    let (token, user) = app.admin().await;
    app.call(
        Method::POST,
        "/api/v1/channels",
        Some(&token),
        Some(json!({ "kind": "webhook", "name": "w", "config": { "url": format!("{}/h", mock.uri()) } })),
    )
    .await;
    let send_token = user["send_token"].as_str().unwrap();
    app.call(
        Method::POST,
        "/api/v1/send",
        Some(send_token),
        Some(json!({ "body": "x" })),
    )
    .await;
    wait_requests(&mock, 1).await;

    let mut stats = Value::Null;
    for _ in 0..50 {
        stats = app
            .call(Method::GET, "/api/v1/stats?days=7", Some(&token), None)
            .await
            .1;
        if stats["kinds"].as_array().is_some_and(|k| !k.is_empty()) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(
        stats["kinds"][0],
        json!({ "kind": "webhook", "name": "自定义 Webhook", "success": 1, "failed": 0 })
    );
    assert_eq!(stats["days"].as_array().unwrap().len(), 1);
    assert_eq!(stats["users"], 1);

    let (status, _) = app.call(Method::GET, "/no/such/file.js", None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn channel_backup_round_trip() {
    let mock = MockServer::start().await;
    let app = app(&mock).await;
    let (token, user) = app.admin().await;
    let hook = json!({ "method": "POST", "url": "https://example.com/hook", "content_type": "json", "body": "{\"t\":\"{title}\"}" });
    let (_, webhook) = app
        .call(
            Method::POST,
            "/api/v1/channels",
            Some(&token),
            Some(json!({ "kind": "webhook", "name": "hook", "config": hook, "active": false })),
        )
        .await;
    let base = format!("/bark/{}", user["send_token"].as_str().unwrap());
    let (status, _) = app
        .send(super::form(&format!("{base}/register"), "device_token=tok1"))
        .await;
    assert_eq!(status, StatusCode::OK);

    let (status, err) = app
        .call(
            Method::POST,
            "/api/v1/channels/export",
            Some(&token),
            Some(json!({ "password": "short" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{err}");
    let (status, backup) = app
        .call(
            Method::POST,
            "/api/v1/channels/export",
            Some(&token),
            Some(json!({ "password": "backup-pass" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{backup}");
    assert_eq!(backup["format"], "inotify-next-channels");
    // Secrets are not readable in the file.
    assert!(!backup.to_string().contains("example.com"));

    let import = |password: &str| json!({ "password": password, "backup": backup.clone() });
    let (status, err) = app
        .call(
            Method::POST,
            "/api/v1/channels/import",
            Some(&token),
            Some(import("wrong-pass")),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{err}");

    // Same user, same server: everything already exists.
    let (status, result) = app
        .call(
            Method::POST,
            "/api/v1/channels/import",
            Some(&token),
            Some(import("backup-pass")),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result, json!({ "imported": 0, "skipped": 2, "rekeyed": 0 }));

    // Deleted channels come back with their key, config and state.
    let uri = format!("/api/v1/channels/{}", webhook["id"]);
    app.call(Method::DELETE, &uri, Some(&token), None).await;
    let (_, result) = app
        .call(
            Method::POST,
            "/api/v1/channels/import",
            Some(&token),
            Some(import("backup-pass")),
        )
        .await;
    assert_eq!(result, json!({ "imported": 1, "skipped": 1, "rekeyed": 0 }));
    let (_, channels) = app.call(Method::GET, "/api/v1/channels", Some(&token), None).await;
    let restored = channels
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["kind"] == "webhook")
        .unwrap();
    assert_eq!(restored["key"], webhook["key"]);
    assert_eq!(restored["config"], hook);
    assert_eq!(restored["active"], false);

    // Another user gets copies under new keys; the Bark device token comes along.
    app.call(
        Method::POST,
        "/api/v1/users",
        Some(&token),
        Some(json!({ "username": "carol", "password": "carol-password" })),
    )
    .await;
    let (carol, _) = app.login("carol", "carol-password").await;
    let (_, result) = app
        .call(
            Method::POST,
            "/api/v1/channels/import",
            Some(&carol),
            Some(import("backup-pass")),
        )
        .await;
    assert_eq!(result, json!({ "imported": 2, "skipped": 0, "rekeyed": 2 }));
    let (_, mine) = app.call(Method::GET, "/api/v1/channels", Some(&carol), None).await;
    let bark = mine.as_array().unwrap().iter().find(|c| c["kind"] == "bark").unwrap();
    assert_eq!(bark["config"]["device_token"], "tok1");
    assert!(channels.as_array().unwrap().iter().all(|c| c["key"] != bark["key"]));
}
