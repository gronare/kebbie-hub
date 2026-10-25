use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::header::{
    ACCEPT, AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE, COOKIE, LOCATION, SET_COOKIE, USER_AGENT,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use kebbie_hub_api::RegisterBegin;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use webauthn_rs::prelude::{Url, Uuid};

use crate::account::{
    ACCOUNT_PREFIX, checked_ticket, connect_ticket, generated_handle, new_id, session_set_cookie,
};
use crate::auth::{Anonymous, SESSION_TOKEN_PREFIX, digest, new_token, random, session_cookie};
use crate::error::ApiError;
use crate::store::{Ceremony, GitHubSignUp, Purpose, SignUpOutcome, Ticket};
use crate::{Hub, clean_name, now};

pub const GITHUB_API_URL: &str = "https://api.github.com";
pub const GITHUB_WEB_URL: &str = "https://github.com";
const STATE_COOKIE: &str = "__Host-kebbie_github";
const DEV_STATE_COOKIE: &str = "kebbie_github";
const SCOPE: &str = "read:user";
const MAX_NEXT_LEN: usize = 512;
const GITHUB_TIMEOUT: Duration = Duration::from_secs(10);
const PURPOSES: [Purpose; 3] = [Purpose::GitHub, Purpose::GitHubConnect, Purpose::GitHubLink];

#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    pub fn new(text: impl Into<String>) -> Secret {
        Secret(text.into())
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Secret(…)")
    }
}

pub struct GitHub {
    client_id: String,
    secret: Secret,
    api: String,
    web: String,
    http: reqwest::Client,
}

pub struct GitHubUser {
    pub id: u64,
    pub login: String,
}

#[derive(Deserialize)]
struct TokenAnswer {
    access_token: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct UserAnswer {
    id: u64,
    login: String,
}

fn checked_base(name: &str, text: &str) -> Result<String, String> {
    let url = Url::parse(text).map_err(|error| format!("{name} does not parse: {error}"))?;
    let loopback = url
        .host_str()
        .is_some_and(|host| host == "localhost" || host.parse::<std::net::IpAddr>().is_ok_and(|address| address.is_loopback()));
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        return Err(format!("{name} must be https, or plain http on a loopback address"));
    }
    Ok(text.trim_end_matches('/').to_string())
}

impl GitHub {
    pub fn new(client_id: &str, secret: Secret, api: &str, web: &str) -> Result<GitHub, String> {
        let http = reqwest::Client::builder()
            .timeout(GITHUB_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| format!("the GitHub client does not build: {error}"))?;
        Ok(GitHub {
            client_id: client_id.to_string(),
            secret,
            api: checked_base("the GitHub API URL", api)?,
            web: checked_base("the GitHub web URL", web)?,
            http,
        })
    }

    fn authorize_url(&self, redirect_uri: &str, state: &str, challenge: &str) -> Result<String, ApiError> {
        Url::parse_with_params(
            &format!("{}/login/oauth/authorize", self.web),
            &[
                ("client_id", self.client_id.as_str()),
                ("redirect_uri", redirect_uri),
                ("scope", SCOPE),
                ("state", state),
                ("code_challenge", challenge),
                ("code_challenge_method", "S256"),
            ],
        )
        .map(String::from)
        .map_err(|_| ApiError::internal())
    }

    async fn token(&self, code: &str, redirect_uri: &str, verifier: &str) -> Result<String, String> {
        let request = serde_json::json!({
            "client_id": self.client_id,
            "client_secret": self.secret.0,
            "code": code,
            "redirect_uri": redirect_uri,
            "code_verifier": verifier,
        });
        let response = self
            .http
            .post(format!("{}/login/oauth/access_token", self.web))
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/json")
            .header(USER_AGENT, "kebbie-hub")
            .body(request.to_string())
            .send()
            .await
            .map_err(|error| format!("the token request failed: {}", error.without_url()))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|_| format!("the token answer did not arrive ({status})"))?;
        let answer: TokenAnswer = serde_json::from_slice(&bytes)
            .map_err(|_| format!("the token answer is not JSON ({status})"))?;
        match (answer.access_token, answer.error) {
            (Some(token), _) if status.is_success() => Ok(token),
            (_, Some(error)) => Err(format!("GitHub refused the code: {}", error.chars().filter(char::is_ascii_graphic).take(64).collect::<String>())),
            _ => Err(format!("GitHub gave no token ({status})")),
        }
    }

    async fn user(&self, code: &str, redirect_uri: &str, verifier: &str) -> Result<GitHubUser, String> {
        let token = self.token(code, redirect_uri, verifier).await?;
        let response = self
            .http
            .get(format!("{}/user", self.api))
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .header(ACCEPT, "application/vnd.github+json")
            .header("x-github-api-version", "2022-11-28")
            .header(USER_AGENT, "kebbie-hub")
            .send()
            .await
            .map_err(|error| format!("the user request failed: {}", error.without_url()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!("GitHub refused the user request ({status})"));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|_| "the user answer did not arrive".to_string())?;
        let user: UserAnswer =
            serde_json::from_slice(&bytes).map_err(|_| "the user answer is not a user".to_string())?;
        let login = clean_name(&user.login)
            .ok_or_else(|| "the user's login is not a name".to_string())?
            .to_string();
        Ok(GitHubUser { id: user.id, login })
    }
}

#[derive(Serialize, Deserialize)]
struct Pending {
    verifier: String,
    browser: String,
    next: String,
    refused: Option<String>,
}

#[derive(Deserialize)]
pub struct Start {
    next: Option<String>,
    connect: Option<String>,
    invite: Option<String>,
    setup: Option<String>,
    link: Option<String>,
}

#[derive(Deserialize)]
pub struct Callback {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

pub fn safe_next(next: Option<&str>) -> String {
    next.filter(|next| {
        next.len() <= MAX_NEXT_LEN
            && next.starts_with('/')
            && !next.starts_with("//")
            && !next.starts_with("/\\")
            && next.chars().all(|char| char.is_ascii_graphic() && char != '#')
    })
    .unwrap_or("/")
    .to_string()
}

fn with_error(next: &str, code: &str) -> String {
    let joiner = if next.contains('?') { '&' } else { '?' };
    format!("{next}{joiner}github_error={code}")
}

fn state_cookie_name(hub: &Hub) -> &'static str {
    if hub.plain_http() {
        DEV_STATE_COOKIE
    } else {
        STATE_COOKIE
    }
}

fn state_set_cookie(hub: &Hub, value: &str, max_age: u64) -> String {
    let name = state_cookie_name(hub);
    let secure = if hub.plain_http() { "" } else { " Secure;" };
    format!("{name}={value}; Path=/; Max-Age={max_age}; HttpOnly;{secure} SameSite=Lax")
}

fn state_cookie<'a>(hub: &Hub, headers: &'a HeaderMap) -> Option<&'a str> {
    let wanted = state_cookie_name(hub);
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == wanted)
        .map(|(_, value)| value.trim())
        .filter(|value| !value.is_empty())
}

fn redirect(location: &str, cookies: &[String]) -> Response {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::SEE_OTHER;
    let headers = response.headers_mut();
    headers.insert(
        LOCATION,
        HeaderValue::from_str(location).unwrap_or(HeaderValue::from_static("/")),
    );
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    for cookie in cookies {
        if let Ok(value) = HeaderValue::from_str(cookie) {
            headers.append(SET_COOKIE, value);
        }
    }
    response
}

fn redirect_uri(hub: &Hub) -> String {
    format!("{}/auth/github/callback", hub.origin())
}

fn challenge_of(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn ticket_for(hub: &Hub, start: &Start) -> Result<(Purpose, Option<Vec<u8>>), ApiError> {
    let begin = RegisterBegin {
        invite: start.invite.clone().unwrap_or_default(),
        setup: start.setup.clone(),
        connect: start.connect.clone(),
    };
    match (&begin.connect, begin.setup.is_some() || !begin.invite.is_empty()) {
        (Some(typed), _) => Ok((Purpose::GitHubConnect, Some(connect_ticket(hub, &begin, typed)?))),
        (None, true) => Ok((Purpose::GitHub, Some(checked_ticket(hub, &begin)?))),
        (None, false) => Ok((Purpose::GitHub, None)),
    }
}

pub async fn start(
    State(hub): State<Arc<Hub>>,
    client: Anonymous,
    headers: HeaderMap,
    Query(start): Query<Start>,
) -> Result<Response, ApiError> {
    hub.limit_anonymous(&client.address)?;
    let Some(github) = hub.github() else {
        return Err(ApiError::not_found());
    };
    let next = safe_next(start.next.as_deref());
    let at = now();
    let (purpose, account, invite_hash, refused) = if start.link.is_some() {
        let account = match session_cookie(&hub, &headers) {
            Some(token) => hub.store().session_account(&digest(token), at)?,
            None => None,
        };
        let Some(account) = account else {
            return Ok(redirect(&with_error(&next, "signed-out"), &[]));
        };
        (Purpose::GitHubLink, Some(account), None, None)
    } else {
        match ticket_for(&hub, &start) {
            Ok((purpose, hash)) => (purpose, None, hash, None),
            Err(refusal) => (Purpose::GitHub, None, None, Some(refusal.code().to_string())),
        }
    };
    let state = new_id();
    let browser = URL_SAFE_NO_PAD.encode(random::<32>());
    let verifier = URL_SAFE_NO_PAD.encode(random::<32>());
    let location = github.authorize_url(&redirect_uri(&hub), &state, &challenge_of(&verifier))?;
    let pending = Pending {
        verifier,
        browser: URL_SAFE_NO_PAD.encode(digest(&browser)),
        next,
        refused,
    };
    hub.store().save_ceremony(&Ceremony {
        id_hash: digest(&state),
        purpose,
        account,
        handle: None,
        user_handle: None,
        invite_hash,
        state: serde_json::to_string(&pending).map_err(|_| ApiError::internal())?,
        expires_at: at + hub.config().ceremony_ttl,
        prf_salt: None,
    })?;
    let cookie = state_set_cookie(&hub, &browser, hub.config().ceremony_ttl);
    Ok(redirect(&location, &[cookie]))
}

enum Outcome {
    SignedIn(String, String),
    Linked(String),
    Refused(&'static str),
}

pub async fn callback(
    State(hub): State<Arc<Hub>>,
    client: Anonymous,
    headers: HeaderMap,
    Query(callback): Query<Callback>,
) -> Result<Response, ApiError> {
    hub.limit_anonymous(&client.address)?;
    let Some(github) = hub.github() else {
        return Err(ApiError::not_found());
    };
    let cleared = state_set_cookie(&hub, "", 0);
    let ceremony = match &callback.state {
        Some(state) => hub.store().take_ceremony(&digest(state), &PURPOSES, now())?,
        None => None,
    };
    let Some(ceremony) = ceremony else {
        return Ok(redirect(&with_error("/", "expired"), &[cleared]));
    };
    let pending: Pending = serde_json::from_str(&ceremony.state).map_err(|_| ApiError::internal())?;
    let same_browser = state_cookie(&hub, &headers)
        .is_some_and(|cookie| URL_SAFE_NO_PAD.encode(digest(cookie)) == pending.browser);
    if !same_browser {
        return Ok(redirect(&with_error(&pending.next, "expired"), &[cleared]));
    }
    if let Some(error) = &callback.error {
        let code = if error == "access_denied" { "denied" } else { "failed" };
        return Ok(redirect(&with_error(&pending.next, code), &[cleared]));
    }
    let Some(code) = &callback.code else {
        return Ok(redirect(&with_error(&pending.next, "failed"), &[cleared]));
    };
    let user = match github.user(code, &redirect_uri(&hub), &pending.verifier).await {
        Ok(user) => user,
        Err(problem) => {
            eprintln!("kebbie-hub: GitHub sign-in failed: {problem}");
            return Ok(redirect(&with_error(&pending.next, "failed"), &[cleared]));
        }
    };
    let outcome = match ceremony.purpose {
        Purpose::GitHubLink => link(&hub, ceremony.account.as_deref(), &user)?,
        _ => sign_in_or_up(&hub, &ceremony, pending.refused.as_deref(), &user)?,
    };
    Ok(match outcome {
        Outcome::SignedIn(account, session) => {
            hub.store().log(now(), "account.logged-in", None, None, Some(&format!("{account} via=github")))?;
            let cookie = session_set_cookie(&hub, &session, hub.config().session_ttl);
            redirect(&pending.next, &[cleared, cookie])
        }
        Outcome::Linked(account) => {
            hub.store().log(now(), "account.github-linked", None, None, Some(&account))?;
            redirect(&pending.next, &[cleared])
        }
        Outcome::Refused(code) => redirect(&with_error(&pending.next, code), &[cleared]),
    })
}

fn link(hub: &Hub, account: Option<&str>, user: &GitHubUser) -> Result<Outcome, ApiError> {
    let Some(account) = account else {
        return Err(ApiError::internal());
    };
    let store = hub.store();
    if store.account(account)?.is_none_or(|row| row.deactivated_at.is_some()) {
        return Ok(Outcome::Refused("signed-out"));
    }
    if !store.link_github(account, user.id, &user.login)? {
        return Ok(Outcome::Refused("github-taken"));
    }
    Ok(Outcome::Linked(account.to_string()))
}

fn sign_in_or_up(
    hub: &Hub,
    ceremony: &Ceremony,
    refused: Option<&str>,
    user: &GitHubUser,
) -> Result<Outcome, ApiError> {
    let session = new_token(SESSION_TOKEN_PREFIX);
    let at = now();
    let expires_at = at + hub.config().session_ttl;
    let mut store = hub.store();
    if let Some(account) = store.account_by_github(user.id)? {
        if store.account(&account)?.is_none_or(|row| row.deactivated_at.is_some()) {
            return Ok(Outcome::Refused("deactivated"));
        }
        store.set_github_login(&account, &user.login)?;
        store.create_session(&digest(&session), &account, at, expires_at)?;
        return Ok(Outcome::SignedIn(account, session));
    }
    let Some(invite_hash) = &ceremony.invite_hash else {
        return Ok(Outcome::Refused(match refused {
            Some("signup-closed") => "signup-closed",
            Some(_) => "unauthorized",
            None => "no-account",
        }));
    };
    let account = format!("{ACCOUNT_PREFIX}{}", new_id());
    let outcome = store.sign_up_github(&GitHubSignUp {
        invite_hash,
        connect: ceremony.purpose == Purpose::GitHubConnect,
        account: &account,
        handle: &generated_handle(),
        user_handle: *Uuid::new_v4().as_bytes(),
        github_id: user.id,
        github_login: &user.login,
        session_hash: &digest(&session),
        session_expires_at: expires_at,
        now: at,
    })?;
    let kind = match outcome {
        SignUpOutcome::Created(kind) => kind,
        SignUpOutcome::InviteSpent => return Ok(Outcome::Refused("unauthorized")),
        SignUpOutcome::GitHubTaken => return Ok(Outcome::Refused("github-taken")),
        SignUpOutcome::HandleTaken | SignUpOutcome::CredentialTaken => {
            return Ok(Outcome::Refused("failed"));
        }
    };
    store.log(
        at,
        "account.created",
        None,
        None,
        Some(&format!("{account} by={} via=github", kind.name())),
    )?;
    drop(store);
    if kind == Ticket::Owner {
        hub.forget_setup();
    }
    Ok(Outcome::SignedIn(account, session))
}

#[cfg(test)]
mod tests {
    use super::{safe_next, with_error};

    #[test]
    fn only_a_path_on_this_hub_is_a_place_to_return_to() {
        assert_eq!(safe_next(Some("/connect?code=K7QF-M2XD")), "/connect?code=K7QF-M2XD");
        for outside in ["//evil.example", "/\\evil.example", "https://evil.example", "connect", "/a#b", "/a b", ""] {
            assert_eq!(safe_next(Some(outside)), "/", "{outside}");
        }
        assert_eq!(safe_next(None), "/");
        assert_eq!(safe_next(Some(&format!("/{}", "a".repeat(600)))), "/");
    }

    #[test]
    fn a_refusal_rides_on_the_return_path_as_a_query() {
        assert_eq!(with_error("/", "denied"), "/?github_error=denied");
        assert_eq!(with_error("/connect?code=X", "no-account"), "/connect?code=X&github_error=no-account");
    }
}
