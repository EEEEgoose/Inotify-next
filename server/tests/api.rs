use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use inotify::{
    api, bootstrap_admin,
    config::{Config, Endpoints},
    db, AppState,
};
use serde_json::{json, Value};
use std::time::Duration;
use tower::ServiceExt;
use wiremock::{
    matchers::{body_json, header as header_is, method, path},
    Mock, MockServer, Request as MockRequest, ResponseTemplate,
};

#[path = "support/more.rs"]
mod more;
#[path = "support/security.rs"]
mod security;

pub struct App {
    pub router: Router,
    admin_password: String,
}

pub async fn app_with(mock: &MockServer, allow_private: bool) -> App {
    let pool = db::connect_memory().await.unwrap();
    let admin_password = bootstrap_admin(&pool).await.unwrap().unwrap();
    let config = Config {
        listen: String::new(),
        data_dir: Default::default(),
        allow_private_targets: allow_private,
        send_concurrency: 4,
        trust_proxy: false,
        disable_turnstile: false,
        endpoints: Endpoints {
            apns: mock.uri(),
            telegram: mock.uri(),
            wecom: mock.uri(),
            turnstile: mock.uri(),
        },
    };
    let state = AppState::new(pool, &config).await.unwrap();
    App {
        router: api::router(state),
        admin_password,
    }
}

pub async fn app(mock: &MockServer) -> App {
    app_with(mock, true).await
}

impl App {
    pub async fn call(
        &self,
        method: Method,
        uri: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(uri);
        if let Some(t) = token {
            req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        let req = match body {
            Some(b) => req
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(b.to_string())),
            None => req.body(Body::empty()),
        }
        .unwrap();
        self.send(req).await
    }

    pub async fn send(&self, req: Request<Body>) -> (StatusCode, Value) {
        let resp = self.router.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let value =
            serde_json::from_slice(&bytes).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into()));
        (status, value)
    }

    pub async fn login(&self, username: &str, password: &str) -> (String, Value) {
        let (status, body) = self
            .call(
                Method::POST,
                "/api/v1/login",
                None,
                Some(json!({ "username": username, "password": password })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        (body["token"].as_str().unwrap().to_string(), body["user"].clone())
    }

    pub async fn admin(&self) -> (String, Value) {
        self.login("admin", &self.admin_password.clone()).await
    }
}

/// Waits until the mock has seen `n` requests (deliveries run in the background).
pub async fn wait_requests(mock: &MockServer, n: usize) -> Vec<MockRequest> {
    for _ in 0..100 {
        let reqs = mock.received_requests().await.unwrap();
        if reqs.len() >= n {
            return reqs;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!(
        "expected {n} requests, got {}",
        mock.received_requests().await.unwrap().len()
    );
}

#[tokio::test]
async fn login_and_sessions() {
    let mock = MockServer::start().await;
    let app = app(&mock).await;

    let (status, _) = app
        .call(
            Method::POST,
            "/api/v1/login",
            None,
            Some(json!({ "username": "admin", "password": "wrong" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = app.call(Method::GET, "/api/v1/me", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = app.call(Method::GET, "/api/v1/me", Some("forged"), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (token, user) = app.admin().await;
    assert_eq!(user["is_admin"], true);
    assert!(user.get("password_hash").is_none());
    let (status, me) = app.call(Method::GET, "/api/v1/me", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["username"], "admin");

    // A normal user cannot administer users; disabling them ends their session at once.
    let (_, bob) = app
        .call(
            Method::POST,
            "/api/v1/users",
            Some(&token),
            Some(json!({ "username": "bob", "password": "bob-password" })),
        )
        .await;
    let (bob_token, _) = app.login("bob", "bob-password").await;
    let (status, _) = app.call(Method::GET, "/api/v1/users", Some(&bob_token), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let uri = format!("/api/v1/users/{}", bob["id"]);
    app.call(Method::PUT, &uri, Some(&token), Some(json!({ "active": false })))
        .await;
    let (status, _) = app.call(Method::GET, "/api/v1/me", Some(&bob_token), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Short passwords are rejected; the old one must be given to change it.
    let (status, _) = app
        .call(
            Method::PUT,
            "/api/v1/me/password",
            Some(&token),
            Some(json!({ "old_password": app.admin_password, "new_password": "short" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn webhook_send_new_and_2x_urls() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/hook"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock)
        .await;
    let app = app(&mock).await;
    let (token, user) = app.admin().await;
    let send_token = user["send_token"].as_str().unwrap();

    // No channel yet.
    let (status, _) = app
        .call(
            Method::POST,
            "/api/v1/send",
            Some(send_token),
            Some(json!({ "title": "x" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, channel) = app
        .call(
            Method::POST,
            "/api/v1/channels",
            Some(&token),
            Some(json!({
                "kind": "webhook",
                "name": "hook",
                "config": { "url": format!("{}/hook", mock.uri()), "body": "{\"t\":\"{title}\",\"b\":\"{body}\"}" }
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{channel}");
    assert_eq!(channel["config"]["method"], "POST", "defaults are filled in");

    let (status, _) = app
        .call(
            Method::POST,
            "/api/v1/send",
            Some(send_token),
            Some(json!({ "title": "say \"hi\"", "body": "line1\nline2" })),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let reqs = wait_requests(&mock, 1).await;
    let got: Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert_eq!(got, json!({ "t": "say \"hi\"", "b": "line1\nline2" }));

    // 2.x URL formats, with the channel key instead of the user token.
    let key = channel["key"].as_str().unwrap();
    for uri in [
        format!("/{key}.send/%E6%A0%87%E9%A2%98/a%2Fb"),
        format!("/api/send?token={key}&title=%E6%A0%87%E9%A2%98&data=a%2Fb"),
        format!("/?act=old/{key}/%E6%A0%87%E9%A2%98/a%2Fb"),
    ] {
        let (status, body) = app.call(Method::GET, &uri, None, None).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {body}");
        assert_eq!(body["code"], 200);
    }
    let reqs = wait_requests(&mock, 4).await;
    for req in &reqs[1..] {
        let got: Value = serde_json::from_slice(&req.body).unwrap();
        assert_eq!(got, json!({ "t": "标题", "b": "a/b" }));
    }

    let (status, _) = app.call(Method::GET, "/nobody.send/x", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn webhook_refuses_private_targets_unless_allowed() {
    let mock = MockServer::start().await;
    let app = app_with(&mock, false).await;
    let (token, _) = app.admin().await;
    let (_, channel) = app
        .call(
            Method::POST,
            "/api/v1/channels",
            Some(&token),
            Some(json!({ "kind": "webhook", "name": "lan", "config": { "url": format!("{}/hook", mock.uri()) } })),
        )
        .await;
    let (_, result) = app
        .call(
            Method::POST,
            &format!("/api/v1/channels/{}/test", channel["id"]),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(result["ok"], false);
    assert!(result["error"].as_str().unwrap().contains("non-public"), "{result}");
    assert!(mock.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn telegram_test_message() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bot123:abc/sendMessage"))
        .and(body_json(
            json!({ "chat_id": "42", "text": "Inotify 测试消息\n渠道「tg」配置正确" }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true })))
        .mount(&mock)
        .await;
    let app = app(&mock).await;
    let (token, _) = app.admin().await;

    let (status, body) = app
        .call(
            Method::POST,
            "/api/v1/channels",
            Some(&token),
            Some(json!({ "kind": "telegram", "name": "tg", "config": { "bot_token": "123:abc" } })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "chat_id is required: {body}");

    let (_, channel) = app
        .call(
            Method::POST,
            "/api/v1/channels",
            Some(&token),
            Some(json!({ "kind": "telegram", "name": "tg", "config": { "bot_token": "123:abc", "chat_id": "42" } })),
        )
        .await;
    let (_, result) = app
        .call(
            Method::POST,
            &format!("/api/v1/channels/{}/test", channel["id"]),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(result, json!({ "ok": true }));
}

pub fn form(uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn bark_register_and_push() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/3/device/aa01"))
        .and(header_is("apns-topic", "me.fin.bark"))
        .and(header_is("apns-push-type", "alert"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock)
        .await;
    Mock::given(method("POST"))
        .and(path("/3/device/bb02"))
        .respond_with(ResponseTemplate::new(410).set_body_json(json!({ "reason": "Unregistered" })))
        .mount(&mock)
        .await;
    let app = app(&mock).await;
    let (token, user) = app.admin().await;
    let base = format!("/bark/{}", user["send_token"].as_str().unwrap());

    let (status, _) = app.call(Method::GET, "/bark/wrong/ping", None, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, pong) = app.call(Method::GET, &format!("{base}/ping"), None, None).await;
    assert_eq!((status, pong["message"].as_str()), (StatusCode::OK, Some("pong")));

    // The app registers without a key and gets one back.
    let (status, reg) = app.send(form(&format!("{base}/register"), "device_token=aa01")).await;
    assert_eq!(status, StatusCode::OK, "{reg}");
    let key = reg["data"]["device_key"].as_str().unwrap().to_string();
    assert_eq!(reg["data"]["key"], key.as_str());
    let (_, channels) = app.call(Method::GET, "/api/v1/channels", Some(&token), None).await;
    assert_eq!(channels.as_array().unwrap().len(), 1);
    assert_eq!(channels[0]["kind"], "bark");
    assert_eq!(channels[0]["config"]["device_token"], "aa01");

    // Users cannot create Bark channels by hand, nor overwrite the device token.
    let (status, _) = app
        .call(
            Method::POST,
            "/api/v1/channels",
            Some(&token),
            Some(json!({ "kind": "bark", "name": "b", "config": {} })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (_, updated) = app
        .call(
            Method::PUT,
            &format!("/api/v1/channels/{}", channels[0]["id"]),
            Some(&token),
            Some(json!({ "config": { "device_token": "evil", "sound": "minuet" } })),
        )
        .await;
    assert_eq!(updated["config"]["device_token"], "aa01");
    assert_eq!(updated["config"]["sound"], "minuet");

    let (status, _) = app
        .call(Method::GET, &format!("{base}/register/{key}"), None, None)
        .await;
    assert_eq!(status, StatusCode::OK);

    // Bark-style push: path title/body plus query params.
    let (status, body) = app
        .call(
            Method::GET,
            &format!("{base}/{key}/%E6%A0%87%E9%A2%98/hello%2Fworld?group=ops&url=https://x.y"),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let reqs = wait_requests(&mock, 1).await;
    let auth = reqs[0].headers.get("authorization").unwrap().to_str().unwrap();
    assert!(auth.starts_with("Bearer "), "{auth}");
    let payload: Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert_eq!(
        payload["aps"]["alert"],
        json!({ "title": "标题", "body": "hello/world" })
    );
    assert_eq!(payload["aps"]["sound"], "minuet.caf");
    assert_eq!(payload["aps"]["thread-id"], "ops");
    assert_eq!(payload["url"], "https://x.y");

    // JSON push to /push.
    let (status, _) = app
        .call(
            Method::POST,
            &format!("{base}/push"),
            None,
            Some(json!({ "device_key": key, "body": "json body" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let reqs = wait_requests(&mock, 2).await;
    let payload: Value = serde_json::from_slice(&reqs[1].body).unwrap();
    assert_eq!(payload["aps"]["alert"], json!({ "body": "json body" }));

    // Re-registering with the same key refreshes the token instead of adding a channel.
    let (_, reg) = app
        .send(form(
            &format!("{base}/register"),
            &format!("device_key={key}&device_token=bb02"),
        ))
        .await;
    assert_eq!(reg["data"]["device_key"], key.as_str());
    let (_, channels) = app.call(Method::GET, "/api/v1/channels", Some(&token), None).await;
    assert_eq!(channels.as_array().unwrap().len(), 1);

    // APNs 410 clears the stale token.
    app.call(Method::GET, &format!("{base}/{key}/bye"), None, None).await;
    wait_requests(&mock, 3).await;
    let mut cleared = false;
    for _ in 0..50 {
        let (_, channels) = app.call(Method::GET, "/api/v1/channels", Some(&token), None).await;
        if channels[0]["config"]["device_token"] == "" {
            cleared = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(cleared, "device token should be cleared after 410");
}

#[tokio::test]
async fn bark_keys_are_scoped_to_their_owner() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock)
        .await;
    let app = app(&mock).await;
    let (admin_token, admin) = app.admin().await;
    app.call(
        Method::POST,
        "/api/v1/users",
        Some(&admin_token),
        Some(json!({ "username": "eve", "password": "eve-password" })),
    )
    .await;
    let (_, eve) = app.login("eve", "eve-password").await;

    let admin_base = format!("/bark/{}", admin["send_token"].as_str().unwrap());
    let eve_base = format!("/bark/{}", eve["send_token"].as_str().unwrap());
    let (_, reg) = app
        .send(form(&format!("{admin_base}/register"), "device_token=ad01"))
        .await;
    let admin_key = reg["data"]["device_key"].as_str().unwrap().to_string();

    // Eve cannot push to, check, or take over the admin's device key.
    let (status, _) = app
        .call(Method::GET, &format!("{eve_base}/{admin_key}/hi"), None, None)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = app
        .call(Method::GET, &format!("{eve_base}/register/{admin_key}"), None, None)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (_, reg) = app
        .send(form(
            &format!("{eve_base}/register"),
            &format!("device_key={admin_key}&device_token=ee02"),
        ))
        .await;
    assert_ne!(reg["data"]["device_key"], admin_key.as_str());

    let (_, channels) = app
        .call(Method::GET, "/api/v1/channels", Some(&admin_token), None)
        .await;
    assert_eq!(channels[0]["config"]["device_token"], "ad01");
}
