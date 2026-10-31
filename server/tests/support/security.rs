//! Sessions, login throttling, Turnstile, response headers and send limits.

use super::{app, form, App};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use serde_json::{json, Value};
use std::time::Duration;
use tower::ServiceExt;
use wiremock::{
    matchers::{body_string_contains, method, path},
    Mock, MockServer, ResponseTemplate,
};

async fn login_status(app: &App, body: Value) -> (StatusCode, Value) {
    app.call(Method::POST, "/api/v1/login", None, Some(body)).await
}

#[tokio::test]
async fn password_changes_end_other_sessions() {
    let mock = MockServer::start().await;
    let app = app(&mock).await;
    let (admin, _) = app.admin().await;
    let (_, bob) = app
        .call(
            Method::POST,
            "/api/v1/users",
            Some(&admin),
            Some(json!({ "username": "bob", "password": "bob-password" })),
        )
        .await;
    let (laptop, _) = app.login("bob", "bob-password").await;
    let (phone, _) = app.login("bob", "bob-password").await;

    // Bob changes his password on the laptop: the laptop continues with a new token, the phone is out.
    let (status, body) = app
        .call(
            Method::PUT,
            "/api/v1/me/password",
            Some(&laptop),
            Some(json!({ "old_password": "bob-password", "new_password": "bob-password-2" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let renewed = body["token"].as_str().unwrap().to_string();
    for (token, expected) in [
        (&laptop, StatusCode::UNAUTHORIZED),
        (&phone, StatusCode::UNAUTHORIZED),
        (&renewed, StatusCode::OK),
    ] {
        let (status, _) = app.call(Method::GET, "/api/v1/me", Some(token), None).await;
        assert_eq!(status, expected);
    }

    // An administrator's reset ends it too; disabling and re-enabling does not bring it back.
    let uri = format!("/api/v1/users/{}", bob["id"]);
    app.call(
        Method::PUT,
        &uri,
        Some(&admin),
        Some(json!({ "password": "reset-password" })),
    )
    .await;
    let (status, _) = app.call(Method::GET, "/api/v1/me", Some(&renewed), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (token, _) = app.login("bob", "reset-password").await;
    app.call(Method::PUT, &uri, Some(&admin), Some(json!({ "active": false })))
        .await;
    app.call(Method::PUT, &uri, Some(&admin), Some(json!({ "active": true })))
        .await;
    let (status, _) = app.call(Method::GET, "/api/v1/me", Some(&token), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn failed_logins_are_throttled() {
    let mock = MockServer::start().await;
    let app = app(&mock).await;
    let wrong = json!({ "username": "admin", "password": "wrong-password" });
    for _ in 0..10 {
        let (status, _) = login_status(&app, wrong.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    // Now even the right password is refused for this client, and unknown users likewise.
    let right = json!({ "username": "admin", "password": app.admin_password });
    let (status, body) = login_status(&app, right).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    let (status, _) = login_status(&app, json!({ "username": "nobody", "password": "x" })).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn turnstile_guards_login() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/turnstile/v0/siteverify"))
        .and(body_string_contains("response=good"))
        .and(body_string_contains("secret=ts-secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true })))
        .mount(&mock)
        .await;
    Mock::given(method("POST"))
        .and(path("/turnstile/v0/siteverify"))
        .and(body_string_contains("response=bad"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "success": false, "error-codes": ["invalid-input-response"] })),
        )
        .mount(&mock)
        .await;
    let app = app(&mock).await;
    let (admin, _) = app.admin().await;

    let (status, _) = app
        .call(
            Method::PUT,
            "/api/v1/settings",
            Some(&admin),
            Some(json!({ "turnstile_enabled": true, "turnstile_site_key": "ts-site" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "the secret key is required");
    let (status, settings) = app
        .call(
            Method::PUT,
            "/api/v1/settings",
            Some(&admin),
            Some(json!({ "turnstile_enabled": true, "turnstile_site_key": "ts-site", "turnstile_secret_key": "ts-secret" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{settings}");
    assert_eq!(settings["turnstile_secret_key"], "");
    assert_eq!(settings["turnstile_secret_key_set"], true);
    let (_, options) = app.call(Method::GET, "/api/v1/auth-options", None, None).await;
    assert_eq!(options["turnstile_site_key"], "ts-site");

    let password = app.admin_password.clone();
    for (token, expected) in [
        (None, StatusCode::BAD_REQUEST),
        (Some("bad"), StatusCode::BAD_REQUEST),
        (Some("good"), StatusCode::OK),
    ] {
        let (status, body) = login_status(
            &app,
            json!({ "username": "admin", "password": password, "turnstile_token": token }),
        )
        .await;
        assert_eq!(status, expected, "{token:?}: {body}");
    }
}

#[tokio::test]
async fn responses_carry_security_headers() {
    let mock = MockServer::start().await;
    let app = app(&mock).await;
    for uri in ["/api/v1/auth-options", "/"] {
        let resp = app
            .router
            .clone()
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let headers = resp.headers();
        let csp = headers[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
        assert!(csp.contains("frame-ancestors 'none'"), "{csp}");
        assert!(csp.contains("https://challenges.cloudflare.com"), "{csp}");
        assert_eq!(headers[header::X_FRAME_OPTIONS], "DENY");
        assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    }
}

#[tokio::test]
async fn send_limits() {
    let mock = MockServer::start().await;
    // A slow target keeps jobs pending.
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
        .mount(&mock)
        .await;
    let app = app(&mock).await;
    let (token, user) = app.admin().await;
    let send_token = user["send_token"].as_str().unwrap().to_string();
    app.call(
        Method::POST,
        "/api/v1/channels",
        Some(&token),
        Some(json!({ "kind": "webhook", "name": "w", "config": { "url": format!("{}/h", mock.uri()) } })),
    )
    .await;
    let send = |body: Value| app.call(Method::POST, "/api/v1/send", Some(&send_token), Some(body));

    let (status, _) = send(json!({ "body": "x".repeat(64 * 1024 + 1) })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let keys = vec!["k"; 101].join(",");
    let (status, _) = app
        .send(form(
            &format!("/bark/{send_token}/push"),
            &format!("device_keys={keys}&body=x"),
        ))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // One user can have at most 500 jobs pending.
    for i in 0..500 {
        let (status, body) = send(json!({ "body": "x" })).await;
        assert_eq!(status, StatusCode::ACCEPTED, "message {i}: {body}");
    }
    let (status, _) = send(json!({ "body": "x" })).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn webhook_errors_do_not_echo_the_response() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500).set_body_string("internal-secret-data"))
        .mount(&mock)
        .await;
    let app = app(&mock).await;
    let (token, _) = app.admin().await;
    let (_, channel) = app
        .call(
            Method::POST,
            "/api/v1/channels",
            Some(&token),
            Some(json!({ "kind": "webhook", "name": "w", "config": { "url": format!("{}/h", mock.uri()) } })),
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
    let error = result["error"].as_str().unwrap();
    assert!(error.contains("500") && !error.contains("secret"), "{error}");
}

#[tokio::test]
async fn bark_rejects_malformed_registrations() {
    let mock = MockServer::start().await;
    let app = app(&mock).await;
    let (_, user) = app.admin().await;
    let base = format!("/bark/{}", user["send_token"].as_str().unwrap());

    let (status, _) = app
        .send(form(&format!("{base}/register"), "device_token=..%2F..%2Fx"))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A malformed key, or one equal to a send token, is replaced.
    for key in ["bad/key", user["send_token"].as_str().unwrap()] {
        let body = format!(
            "device_key={}&device_token=aa01",
            url::form_urlencoded::byte_serialize(key.as_bytes()).collect::<String>()
        );
        let (status, reg) = app.send(form(&format!("{base}/register"), &body)).await;
        assert_eq!(status, StatusCode::OK, "{reg}");
        assert_ne!(reg["data"]["device_key"], key);
    }
}

#[tokio::test]
async fn only_administrators_create_users() {
    let mock = MockServer::start().await;
    let app = app(&mock).await;
    let (admin, _) = app.admin().await;
    let new_user = |name: &str| json!({ "username": name, "password": "a-good-password" });
    app.call(Method::POST, "/api/v1/users", Some(&admin), Some(new_user("bob")))
        .await;
    let (bob, _) = app.login("bob", "a-good-password").await;

    let (status, _) = app
        .call(Method::POST, "/api/v1/users", None, Some(new_user("eve")))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = app
        .call(Method::POST, "/api/v1/users", Some(&bob), Some(new_user("eve")))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // Nor can a user make themselves an administrator.
    let (status, _) = app
        .call(
            Method::PUT,
            "/api/v1/users/2",
            Some(&bob),
            Some(json!({ "is_admin": true })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (_, users) = app.call(Method::GET, "/api/v1/users", Some(&admin), None).await;
    let names: Vec<&str> = users
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["username"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["admin", "bob"]);
}
