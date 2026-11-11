#[allow(dead_code)]
mod common;

use common::*;
use kebbie_hub_api::{AccountView, CreationCeremony, LoginFinish};
use kebbie_hub_fakes::passkey::DiscoverablePasskey;
use serde_json::Value;
use webauthn_authenticator_rs::WebauthnAuthenticator;
use webauthn_authenticator_rs::prelude::WebauthnCError;

async fn finish_discoverable(person: &mut Person) -> Result<reqwest::Response, WebauthnCError> {
    let ceremony = person.begin_log_in().await;
    let credential = person.passkey.do_authentication(origin(), ceremony.options)?;
    let response = person
        .post(
            "/v1/a/login/finish",
            &LoginFinish {
                ceremony: ceremony.ceremony,
                credential,
            },
        )
        .await;
    if response.status().is_success() {
        person.keep_cookie(&response);
    }
    Ok(response)
}

#[tokio::test]
async fn sign_up_asks_for_a_discoverable_passkey_with_user_verification() {
    let test = start_hub(|_| {}).await;
    let person = Person::new(&test.url);
    let ceremony: Value = body(person.begin_sign_up(&test.hub.mint_invite().unwrap()).await).await;
    let selection = &ceremony["options"]["publicKey"]["authenticatorSelection"];
    assert_eq!(selection["residentKey"], "required");
    assert_eq!(selection["requireResidentKey"], true);
    assert_eq!(selection["userVerification"], "required");
}

#[tokio::test]
async fn a_passkey_signs_in_without_the_account_name_and_the_hub_finds_the_account_from_it() {
    let test = start_hub(|_| {}).await;
    let mut person = signed_up(&test).await;
    let account: AccountView = body(person.get("/v1/a/account").await).await;
    person.cookie = None;
    let ceremony: Value = body(
        person
            .post("/v1/a/login/begin", &serde_json::json!({}))
            .await,
    )
    .await;
    let options = &ceremony["options"];
    assert!(options["publicKey"]["allowCredentials"].as_array().is_none_or(Vec::is_empty), "{options}");
    assert!(options.get("mediation").is_none(), "the page picks the mediation: a button or autofill");

    let response = finish_discoverable(&mut person).await.unwrap();
    let view: AccountView = body(response).await;
    assert_eq!(view.account, account.account);
    assert!(person.cookie.is_some());
    assert!(person.get("/v1/a/account").await.status().is_success());
}

#[tokio::test]
async fn a_passkey_of_an_account_this_hub_does_not_have_is_refused_as_unknown() {
    let elsewhere = start_hub(|_| {}).await;
    let mut person = signed_up(&elsewhere).await;
    let test = start_hub(|_| {}).await;
    person.url = test.url.clone();
    person.cookie = None;
    let response = finish_discoverable(&mut person).await.unwrap();
    assert_eq!(refusal(response).await, (401, "unknown-passkey".to_string()));
    assert!(person.cookie.is_none());
}

#[tokio::test]
async fn a_passkey_the_authenticator_does_not_offer_cannot_sign_in_but_a_session_can_add_one_it_does() {
    let test = start_hub(|_| {}).await;
    let mut person = Person::new(&test.url);
    person.passkey = WebauthnAuthenticator::new(DiscoverablePasskey::without_resident_keys(true));
    let ceremony: CreationCeremony = body(person.begin_sign_up(&test.hub.mint_invite().unwrap()).await).await;
    assert!(person.finish_sign_up(ceremony).await.status().is_success());
    let session = person.cookie.clone();
    person.cookie = None;
    assert!(matches!(finish_discoverable(&mut person).await, Err(WebauthnCError::NoSelectedToken)), "the authenticator offers nothing");

    person.cookie = session;
    let mut fresh = WebauthnAuthenticator::new(DiscoverablePasskey::new(true));
    person.add_passkey(&mut fresh).await;
    person.passkey = fresh;
    person.cookie = None;
    let response = finish_discoverable(&mut person).await.unwrap();
    assert!(response.status().is_success(), "{}", response.status());
}

#[tokio::test]
async fn a_deactivated_account_cannot_sign_in_without_its_name_either() {
    let test = start_hub(|_| {}).await;
    let mut person = signed_up(&test).await;
    let account: AccountView = body(person.get("/v1/a/account").await).await;
    test.hub
        .store()
        .set_deactivated(&account.account, Some(1))
        .unwrap();
    person.cookie = None;
    let response = finish_discoverable(&mut person).await.unwrap();
    assert_eq!(refusal(response).await, (401, "unauthorized".to_string()));
}
