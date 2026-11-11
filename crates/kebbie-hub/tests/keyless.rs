#[allow(dead_code)]
mod common;

use std::sync::Arc;

use axum::extract::State;
use axum::http::header::SET_COOKIE;
use axum::response::{AppendHeaders, IntoResponse};
use axum::routing::post;
use axum::{Json, Router};
use common::*;
use kebbie_hub::{ApiError, Hooks, Hub, sign_in_without_key, sign_up_without_key};
use kebbie_hub_api::{AccountView, HubInfo, RegisterBegin, SignupMode};
use kebbie_hub_fakes::machine::connect;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
struct Proven {
    account: Option<String>,
    connect: Option<String>,
}

async fn proven(State(hub): State<Arc<Hub>>, Json(proven): Json<Proven>) -> Result<impl IntoResponse, ApiError> {
    let signed = match (proven.account, proven.connect) {
        (Some(account), _) => sign_in_without_key(&hub, &account, "test")?,
        (None, Some(code)) => sign_up_without_key(&hub, &code, "test")?,
        (None, None) => return Err(ApiError::bad_request("nothing-proven")),
    };
    Ok((AppendHeaders([(SET_COOKIE, signed.set_cookie)]), Json(signed.account)))
}

fn embedder() -> Router<Arc<Hub>> {
    Router::new().route("/test/proven", post(proven))
}

async fn embedding_hub(email_sign_in: bool) -> TestHub {
    start_hub_hooks(
        |config| config.signup = SignupMode::Open,
        Hooks { email_sign_in, ..Hooks::default() },
        embedder(),
    )
    .await
}

async fn keyless_session(test: &TestHub, account: &str) -> kebbie_hub_fakes::browser::Browser {
    let mut other = browser(test);
    let view: AccountView = other.post("/test/proven", &json!({ "account": account })).await.unwrap();
    assert!(!view.key_access);
    other
}

#[tokio::test]
async fn a_default_hub_never_offers_or_takes_email_sign_in() {
    let test = start_hub(|config| config.signup = SignupMode::Open).await;
    let mut anonymous = browser(&test);
    let info: HubInfo = anonymous.get("/v1/hub").await.unwrap();
    assert!(!info.email_signin && !info.email);
    for path in ["/v1/a/email/code", "/v1/a/email/verify"] {
        let refused = anonymous.post::<_, Value>(path, &json!({ "email": "ada@example.com" })).await;
        let (status, _) = hub_code(refused.unwrap_err());
        assert!(status == 404 || status == 405, "{path} answered {status}");
    }
    let pending = connect(&test.url, "mac").await.unwrap();
    let mut person = browser(&test);
    let view = person.sign_up_from_connect(&pending.started.verification_uri_complete).await.unwrap();
    assert!(view.key_access, "a passkey sign-up still makes an account");
}

#[tokio::test]
async fn a_session_without_key_access_reaches_the_account_and_nothing_the_key_guards() {
    let test = embedding_hub(true).await;
    let mut person = member(&test).await;
    let me = person.account().await.unwrap();
    assert!(me.key.is_some() && me.key_access);
    let mut keyless = keyless_session(&test, &me.account).await;
    let seen = keyless.account().await.unwrap();
    assert_eq!((seen.account.as_str(), seen.key_access, seen.key.is_some()), (me.account.as_str(), false, true));

    let passkey = person.get::<Value>("/v1/a/passkeys").await.unwrap()["passkeys"][0]["passkey"].as_str().unwrap().to_string();
    let wrapper = json!({ "key_version": 1, "wrapped": "AA", "signature": "AA" });
    let refused = [
        keyless.get::<Value>("/v1/a/boards").await.map(drop),
        keyless.post::<_, Value>("/v1/a/commands", &json!({ "envelope": {} })).await.map(drop),
        keyless.get::<Value>("/v1/a/results").await.map(drop),
        keyless.post::<_, Value>("/v1/a/key", &json!({})).await.map(drop),
        keyless.put::<_, Value>(&format!("/v1/a/passkeys/{passkey}/wrapper"), &wrapper).await.map(drop),
        keyless.post::<_, Value>("/v1/a/passkeys/begin", &()).await.map(drop),
        keyless.delete(&format!("/v1/a/passkeys/{passkey}")).await,
        keyless.get::<Value>("/v1/a/events").await.map(drop),
        keyless.get::<Value>("/v1/a/pairings/whatever").await.map(drop),
    ];
    for (index, result) in refused.into_iter().enumerate() {
        let (status, code) = hub_code(result.expect_err("a keyless session is refused"));
        assert_eq!((status, code.as_str()), (403, "passkey-required"), "call {index}");
    }
    assert_eq!(rows(&test, "commands"), 0, "no command was queued");
    assert_eq!(person.get::<Value>("/v1/a/passkeys").await.unwrap()["passkeys"].as_array().unwrap().len(), 1);
    keyless.get::<Value>("/v1/a/passkeys").await.unwrap();
}

#[tokio::test]
async fn only_the_first_passkey_of_an_account_without_a_key_lifts_its_session() {
    let test = embedding_hub(true).await;
    let pending = connect(&test.url, "mac").await.unwrap();
    let mut arriving = browser(&test);
    let created: AccountView = arriving.post("/test/proven", &json!({ "connect": pending.started.user_code })).await.unwrap();
    assert!(!created.key_access && created.key.is_none());
    assert_eq!(created_via(&test, &created.account).as_deref(), Some("connect"));
    let mut second = keyless_session(&test, &created.account).await;

    arriving.add_passkey().await.unwrap();
    assert!(arriving.account().await.unwrap().key_access, "the first passkey lifts the session that made it");
    let late = second.post::<_, Value>("/v1/a/passkeys/begin", &()).await.map(drop);
    assert!(late.is_ok(), "the account still has no key, so a first passkey is still open");
    let raced = second.add_passkey().await.unwrap();
    assert!(!raced.unlocks);
    assert!(second.account().await.unwrap().key_access);
}

#[tokio::test]
async fn a_hub_that_signs_in_by_email_takes_no_bare_passkey_connect_sign_up() {
    let test = embedding_hub(true).await;
    let info: HubInfo = browser(&test).get("/v1/hub").await.unwrap();
    assert!(info.email_signin);
    let pending = connect(&test.url, "mac").await.unwrap();
    let mut person = browser(&test);
    let refused = person
        .post::<_, Value>("/v1/a/register/begin", &RegisterBegin { connect: Some(pending.started.user_code.clone()), ..RegisterBegin::default() })
        .await;
    assert_eq!(hub_code(refused.unwrap_err()), (403, "email-required".to_string()));
    let invite = test.hub.mint_invite().unwrap();
    person.sign_up_with_invite(&invite).await.unwrap();
}

#[tokio::test]
async fn a_deactivated_account_opens_no_keyless_session() {
    let test = embedding_hub(true).await;
    let mut person = member(&test).await;
    let account = person.account().await.unwrap().account;
    kebbie_hub::set_account_active(&test.hub.store(), &account, false, "test").unwrap();
    let refused = browser(&test).post::<_, AccountView>("/test/proven", &json!({ "account": account })).await;
    assert_eq!(hub_code(refused.unwrap_err()).0, 403);
}
