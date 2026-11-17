#[allow(dead_code)]
mod common;

use common::*;
use kebbie_hub::{ACCOUNT_HEADER, DEV_SESSION_COOKIE};
use kebbie_hub_api::AccountView;
use reqwest::Method;

async fn account_as(person: &Person, account: Option<&str>) -> reqwest::Response {
    let mut request = person.request(Method::GET, "/v1/a/account");
    if let Some(account) = account {
        request = request.header(ACCOUNT_HEADER, account);
    }
    request.send().await.unwrap()
}

async fn two_accounts(test: &TestHub) -> (Person, String, String) {
    let home = signed_up(test).await;
    let home_id = body::<AccountView>(home.get("/v1/a/account").await).await.account;
    let mut both = signed_up(test).await;
    let work_id = body::<AccountView>(both.get("/v1/a/account").await).await.account;
    both.cookie = home.cookie.clone();
    assert_eq!(both.log_in().await.status(), 200);
    (both, home_id, work_id)
}

#[tokio::test]
async fn a_cookie_from_before_multiple_accounts_still_signs_in_with_or_without_naming_the_account() {
    let test = start_hub(|_| {}).await;
    let person = signed_up(&test).await;
    let account = body::<AccountView>(person.get("/v1/a/account").await).await.account;
    let cookie = person.cookie.clone().unwrap();
    assert!(cookie.starts_with(&format!("{DEV_SESSION_COOKIE}=khs_")) && !cookie.contains('.'), "{cookie}");
    let mut legacy = Person::new(&test.url);
    legacy.cookie = Some(cookie);
    assert_eq!(body::<AccountView>(account_as(&legacy, None).await).await.account, account);
    assert_eq!(body::<AccountView>(account_as(&legacy, Some(&account)).await).await.account, account);
    assert_eq!(account_as(&legacy, Some("acc_someoneelse")).await.status(), 401);
}

#[tokio::test]
async fn a_second_sign_in_keeps_the_first_account_and_each_request_reaches_the_account_it_names() {
    let test = start_hub(|_| {}).await;
    let (both, home, work) = two_accounts(&test).await;
    let cookie = both.cookie.clone().unwrap();
    assert_eq!(cookie.split_once('=').unwrap().1.split('.').count(), 2, "{cookie}");
    assert_eq!(body::<AccountView>(account_as(&both, Some(&home)).await).await.account, home);
    assert_eq!(body::<AccountView>(account_as(&both, Some(&work)).await).await.account, work);
    assert_eq!(refusal(account_as(&both, None).await).await, (400, "account-required".to_string()));
    let by_query = both.get(&format!("/v1/a/account?account={home}")).await;
    assert_eq!(body::<AccountView>(by_query).await.account, home);
}

#[tokio::test]
async fn signing_in_again_to_a_held_account_replaces_its_session_instead_of_adding_one() {
    let test = start_hub(|_| {}).await;
    let (mut both, home, _) = two_accounts(&test).await;
    assert_eq!(both.log_in().await.status(), 200);
    let cookie = both.cookie.clone().unwrap();
    assert_eq!(cookie.split_once('=').unwrap().1.split('.').count(), 2, "{cookie}");
    assert_eq!(account_as(&both, Some(&home)).await.status(), 200);
}

#[tokio::test]
async fn signing_out_of_one_account_keeps_the_other_and_signing_out_unnamed_ends_both() {
    let test = start_hub(|_| {}).await;
    let (mut both, home, work) = two_accounts(&test).await;
    let held = both.cookie.clone();
    let out = both.request(Method::POST, "/v1/a/logout").header(ACCOUNT_HEADER, &work).json(&()).send().await.unwrap();
    assert_eq!(out.status(), 204);
    both.keep_cookie(&out);
    assert_eq!(account_as(&both, Some(&work)).await.status(), 401);
    assert_eq!(body::<AccountView>(account_as(&both, None).await).await.account, home);

    let mut old_app = Person::new(&test.url);
    old_app.cookie = held;
    let out = old_app.post("/v1/a/logout", &()).await;
    assert_eq!(out.status(), 204);
    assert!(old_app.keep_cookie(&out).contains("Max-Age=0"));
    assert_eq!(account_as(&both, Some(&home)).await.status(), 401);
}
