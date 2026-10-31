//! GitHub login. Accounts are matched by GitHub's numeric user id only, never by username,
//! so a GitHub user named like a local account cannot take it over.

use crate::{
    auth::AuthUser,
    db::{self, User},
    error::{AppError, AppResult},
    AppState,
};
use anyhow::Context;
use axum::{
    extract::{Query, State},
    http::{header, HeaderMap},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const COOKIE: &str = "inotify_oauth";
const STATE_TTL: Duration = Duration::from_secs(600);
const CALLBACK_PATH: &str = "/api/v1/oauth/github/callback";

pub async fn auth_options(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "github": state.settings().github_ready() }))
}

/// External base URL: the configured public URL, or what the request (and a reverse proxy) says.
pub fn base_url(state: &AppState, headers: &HeaderMap) -> String {
    let configured = state.settings().public_url;
    if !configured.is_empty() {
        return configured.trim_end_matches('/').to_string();
    }
    let get = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    let host = get("x-forwarded-host").or_else(|| get("host")).unwrap_or("localhost");
    let proto = get("x-forwarded-proto").unwrap_or("http");
    format!("{proto}://{host}")
}

pub fn callback_url(state: &AppState, headers: &HeaderMap) -> String {
    format!("{}{CALLBACK_PATH}", base_url(state, headers))
}

fn new_state(state: &AppState, link_user: Option<i64>) -> String {
    let token = db::random_token(32);
    let mut states = state.oauth_states.lock().unwrap();
    states.retain(|_, (_, at)| at.elapsed() < STATE_TTL);
    states.insert(token.clone(), (link_user, Instant::now()));
    token
}

/// Starts linking GitHub to the logged-in account; the browser then opens the returned URL.
pub async fn link(State(state): State<AppState>, AuthUser(user): AuthUser) -> AppResult<Json<Value>> {
    if !state.settings().github_ready() {
        return Err(AppError::BadRequest("未启用 GitHub 登录".into()));
    }
    let token = new_state(&state, Some(user.id));
    Ok(Json(
        json!({ "url": format!("/api/v1/oauth/github/start?state={token}") }),
    ))
}

#[derive(Deserialize)]
pub struct StartQuery {
    state: Option<String>,
}

/// Redirects to GitHub. The state is also set as a cookie so the callback only completes in the
/// browser that started the flow.
pub async fn start(State(state): State<AppState>, headers: HeaderMap, Query(q): Query<StartQuery>) -> Response {
    let settings = state.settings();
    if !settings.github_ready() {
        return fail_redirect("未启用 GitHub 登录");
    }
    let token = match q.state {
        Some(s) if state.oauth_states.lock().unwrap().contains_key(&s) => s,
        Some(_) => return fail_redirect("登录已过期，请重试"),
        None => new_state(&state, None),
    };
    let mut url = url::Url::parse(&format!("{}/login/oauth/authorize", state.endpoints.github_web)).unwrap();
    url.query_pairs_mut()
        .append_pair("client_id", &settings.github_client_id)
        .append_pair("redirect_uri", &callback_url(&state, &headers))
        .append_pair("state", &token)
        .append_pair("allow_signup", "false");
    let secure = if base_url(&state, &headers).starts_with("https://") {
        "; Secure"
    } else {
        ""
    };
    let cookie = format!(
        "{COOKIE}={token}; Path=/api/v1/oauth; Max-Age={}; HttpOnly; SameSite=Lax{secure}",
        STATE_TTL.as_secs()
    );
    ([(header::SET_COOKIE, cookie)], Redirect::to(url.as_str())).into_response()
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
}

fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v)
}

fn fail_redirect(message: &str) -> Response {
    let message: String = url::form_urlencoded::byte_serialize(message.as_bytes()).collect();
    Redirect::to(&format!("/#/login?error={message}")).into_response()
}

pub async fn callback(State(state): State<AppState>, headers: HeaderMap, Query(q): Query<CallbackQuery>) -> Response {
    let (Some(code), Some(token)) = (q.code, q.state) else {
        return fail_redirect("GitHub 授权被取消");
    };
    if cookie_value(&headers, COOKIE) != Some(token.as_str()) {
        return fail_redirect("登录状态校验失败，请重试");
    }
    let pending = state.oauth_states.lock().unwrap().remove(&token);
    let link_user = match pending {
        Some((link_user, at)) if at.elapsed() < STATE_TTL => link_user,
        _ => return fail_redirect("登录已过期，请重试"),
    };
    let github = match fetch_github_user(&state, &code, &callback_url(&state, &headers)).await {
        Ok(user) => user,
        Err(e) => {
            tracing::warn!("GitHub OAuth failed: {e:#}");
            return fail_redirect("GitHub 登录失败");
        }
    };
    match finish(&state, link_user, github).await {
        Ok(Finish::Linked) => Redirect::to("/#/account?github=linked").into_response(),
        Ok(Finish::LoggedIn(user)) => match state.jwt.issue(user.id) {
            // The fragment never reaches server logs; the web app picks the token up from it.
            Ok(jwt) => Redirect::to(&format!("/#/oauth?token={jwt}")).into_response(),
            Err(e) => {
                tracing::error!("issue session: {e:#}");
                fail_redirect("登录失败")
            }
        },
        Err(AppError::BadRequest(message)) => fail_redirect(&message),
        Err(e) => {
            tracing::error!("GitHub login: {e}");
            fail_redirect("登录失败")
        }
    }
}

struct GithubUser {
    id: i64,
    login: String,
    avatar_url: Option<String>,
}

async fn fetch_github_user(state: &AppState, code: &str, redirect_uri: &str) -> anyhow::Result<GithubUser> {
    let settings = state.settings();
    let client = state.send_ctx.net.client();
    let token: Value = client
        .post(format!("{}/login/oauth/access_token", state.endpoints.github_web))
        .header(header::ACCEPT, "application/json")
        .form(&[
            ("client_id", settings.github_client_id.as_str()),
            ("client_secret", settings.github_client_secret.as_str()),
            ("code", code),
            ("redirect_uri", redirect_uri),
        ])
        .send()
        .await?
        .json()
        .await?;
    let access_token = token["access_token"]
        .as_str()
        .with_context(|| format!("token exchange failed: {}", token["error"]))?;
    let user: Value = client
        .get(format!("{}/user", state.endpoints.github_api))
        .bearer_auth(access_token)
        .header(header::USER_AGENT, "inotify")
        .header(header::ACCEPT, "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(GithubUser {
        id: user["id"].as_i64().context("GitHub user has no id")?,
        login: user["login"].as_str().context("GitHub user has no login")?.to_string(),
        avatar_url: user["avatar_url"].as_str().map(String::from),
    })
}

enum Finish {
    Linked,
    LoggedIn(User),
}

async fn finish(state: &AppState, link_user: Option<i64>, github: GithubUser) -> AppResult<Finish> {
    let existing: Option<User> = sqlx::query_as("SELECT * FROM users WHERE github_id = ?")
        .bind(github.id)
        .fetch_optional(&state.db)
        .await?;

    if let Some(user_id) = link_user {
        if existing.as_ref().is_some_and(|u| u.id != user_id) {
            return Err(AppError::BadRequest("该 GitHub 账号已绑定其他用户".into()));
        }
        set_github(state, user_id, &github).await?;
        return Ok(Finish::Linked);
    }

    if let Some(user) = existing {
        if !user.active {
            return Err(AppError::BadRequest("用户已被禁用".into()));
        }
        set_github(state, user.id, &github).await?;
        return Ok(Finish::LoggedIn(user));
    }
    if !state.settings().github_allow_signup {
        return Err(AppError::BadRequest(
            "该 GitHub 账号未绑定用户，请先用密码登录后在账号页绑定".into(),
        ));
    }
    // Pick a free username based on the GitHub login; the account starts without a password.
    let mut username = github.login.clone();
    for n in 2.. {
        if db::user_by_name(&state.db, &username).await?.is_none() {
            break;
        }
        username = format!("{}-{n}", github.login);
    }
    let user = db::create_user(&state.db, &username, None, false).await?;
    set_github(state, user.id, &github).await?;
    Ok(Finish::LoggedIn(user))
}

async fn set_github(state: &AppState, user_id: i64, github: &GithubUser) -> sqlx::Result<()> {
    sqlx::query("UPDATE users SET github_id = ?, github_login = ?, avatar_url = ? WHERE id = ?")
        .bind(github.id)
        .bind(&github.login)
        .bind(&github.avatar_url)
        .bind(user_id)
        .execute(&state.db)
        .await?;
    Ok(())
}
