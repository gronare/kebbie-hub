#[allow(dead_code)]
mod common;

use common::*;
use kebbie_hub::{Config, Hub};
use kebbie_hub_api::{HubInfo, InviteLink, RegisterBegin, SignupMode};

#[tokio::test]
async fn a_fresh_hub_is_claimed_only_with_its_setup_link() {
    let test = start_hub(|_| {}).await;
    let setup = test.hub.setup_url().unwrap();
    assert!(
        setup.starts_with("http://localhost:8470/setup?token=kho_"),
        "{setup}"
    );
    let mut anonymous = browser(&test);
    let info: HubInfo = anonymous.get("/v1/hub").await.unwrap();
    assert_eq!(
        info,
        HubInfo {
            signup: SignupMode::Invite,
            plans: false,
            needs_setup: true,
            github: false
        }
    );
    let token = setup.split_once("token=").unwrap().1.to_string();
    let mut guesser = browser(&test);
    for wrong in [format!("{token}x"), "kho_guess".to_string(), String::new()] {
        let refused = guesser
            .sign_up(RegisterBegin {
                setup: Some(wrong),
                ..RegisterBegin::default()
            })
            .await;
        assert_eq!(hub_code(refused.unwrap_err()).0, 401);
    }
    let without = guesser.sign_up(RegisterBegin::default()).await;
    assert_eq!(hub_code(without.unwrap_err()).0, 401);
    assert!(!holds(&database_bytes(&test), token.as_bytes()));

    let mut first = browser(&test);
    let created = first.sign_up_as_owner(&setup).await.unwrap();
    assert!(created.owner);
    assert_eq!(test.hub.setup_url(), None);
    let mut second = browser(&test);
    assert_eq!(
        hub_code(second.sign_up_as_owner(&setup).await.unwrap_err()).0,
        401
    );
    let info: HubInfo = anonymous.get("/v1/hub").await.unwrap();
    assert!(!info.needs_setup);
}

#[tokio::test]
async fn a_hub_mints_a_new_setup_link_at_each_start_until_it_has_an_account() {
    let data = tempfile::tempdir().unwrap();
    let first = Hub::open(Config::new(data.path()))
        .unwrap()
        .setup_url()
        .unwrap();
    let second = Hub::open(Config::new(data.path()))
        .unwrap()
        .setup_url()
        .unwrap();
    assert_ne!(first, second);
    let connection = rusqlite::Connection::open(kebbie_hub::database_path(data.path())).unwrap();
    let held: u64 = connection
        .query_row(
            "SELECT count(*) FROM invites WHERE kind = 'owner'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(held, 1, "the earlier link stopped working");
    let test = start_hub(|_| {}).await;
    member(&test).await;
    let reopened = Hub::open(Config::new(test.data.path())).unwrap();
    assert_eq!(reopened.setup_url(), None);
}

#[tokio::test]
async fn the_owner_hands_out_invite_links() {
    let test = start_hub(|_| {}).await;
    let mut owner = owner(&test).await;
    let link: InviteLink = owner.post("/v1/a/admin/invites", &()).await.unwrap();
    assert_eq!(
        link.url,
        format!("http://localhost:8470/signup?invite={}", link.code)
    );
    let mut invited = browser(&test);
    let created = invited.sign_up_with_invite(&link.code).await.unwrap();
    assert!(!created.owner);
    let mut again = browser(&test);
    assert_eq!(
        hub_code(again.sign_up_with_invite(&link.code).await.unwrap_err()).0,
        401
    );
    let mut member = invited;
    let refused = member
        .post::<_, InviteLink>("/v1/a/admin/invites", &())
        .await;
    assert_eq!(
        hub_code(refused.unwrap_err()),
        (403, "not-owner".to_string())
    );
}

#[tokio::test]
async fn the_hub_takes_no_email_sign_ups() {
    let test = start_hub(|config| config.signup = SignupMode::Open).await;
    let mut person = browser(&test);
    for path in ["/v1/a/signup/email", "/v1/a/signup/verify?token=khe_x"] {
        let refused = person.get::<serde_json::Value>(path).await;
        assert_eq!(hub_code(refused.unwrap_err()).0, 404, "{path}");
    }
}

#[tokio::test]
async fn an_open_hub_makes_an_account_from_a_pending_connect_without_a_name() {
    let test = start_hub(|config| config.signup = SignupMode::Open).await;
    let pending = kebbie_hub_fakes::machine::connect(&test.url, "mac")
        .await
        .unwrap();
    let code = pending.started.user_code.clone();
    let mut person = browser(&test);
    let shown: kebbie_hub_api::ConnectRequestView = person
        .get(&format!("/v1/connect?code={code}"))
        .await
        .unwrap();
    assert_eq!(shown.name, "mac");

    let created = person.sign_up_from_connect(&pending.link()).await.unwrap();
    assert!(!created.owner);
    assert!(
        created.handle.starts_with('k') && created.handle.len() == 13,
        "{}",
        created.handle
    );
    let mut second = browser(&test);
    assert_eq!(
        hub_code(second.sign_up_from_connect(&pending.link()).await.unwrap_err()).0,
        401
    );

    let approval = person.approve_connect(&pending.link(), true).await.unwrap();
    let (mut machine, pairing, secret) = pending.wait(WAIT).await.unwrap();
    let pairing = pairing.expect("the approval opened a handoff");
    let (pinned, confirmed) = tokio::join!(
        machine.finish_connect(&pairing, &secret, WAIT),
        person.hand_off(approval, WAIT)
    );
    pinned.unwrap();
    assert_eq!(confirmed.unwrap(), machine.machine);
}

#[tokio::test]
async fn an_invite_hub_makes_no_account_from_a_connect() {
    let test = start_hub(|_| {}).await;
    let pending = kebbie_hub_fakes::machine::connect(&test.url, "mac")
        .await
        .unwrap();
    let mut person = browser(&test);
    assert_eq!(
        hub_code(person.sign_up_from_connect(&pending.link()).await.unwrap_err()),
        (403, "signup-closed".to_string())
    );
}

#[tokio::test]
async fn a_connect_code_that_is_not_pending_makes_no_account() {
    let test = start_hub(|config| config.signup = SignupMode::Open).await;
    let mut person = browser(&test);
    assert_eq!(
        hub_code(
            person
                .sign_up_from_connect("http://localhost:8470/connect?code=K7QF-M2XD")
                .await
                .unwrap_err()
        )
        .0,
        401
    );
    let pending = kebbie_hub_fakes::machine::connect(&test.url, "mac")
        .await
        .unwrap();
    let mut approver = member(&test).await;
    approver.approve_connect(&pending.link(), false).await.unwrap();
    assert_eq!(
        hub_code(person.sign_up_from_connect(&pending.link()).await.unwrap_err()).0,
        401
    );
}

#[tokio::test]
async fn an_invite_signs_up_without_an_account_name() {
    let test = start_hub(|_| {}).await;
    let mut person = browser(&test);
    let created = person
        .sign_up_with_invite(&test.hub.mint_invite().unwrap())
        .await
        .unwrap();
    assert!(created.handle.starts_with('k'), "{}", created.handle);
}

#[tokio::test]
async fn an_account_keeps_the_ticket_it_was_made_with() {
    let test = start_hub(|config| config.signup = SignupMode::Open).await;
    let owner = owner(&test).await;
    let invited = member(&test).await;
    let pending = kebbie_hub_fakes::machine::connect(&test.url, "mac")
        .await
        .unwrap();
    let mut connecting = browser(&test);
    let connected = connecting.sign_up_from_connect(&pending.link()).await.unwrap();
    for (mut person, via) in [(owner, "setup"), (invited, "invite")] {
        let account = person.account().await.unwrap().account;
        assert_eq!(created_via(&test, &account).as_deref(), Some(via));
    }
    assert_eq!(created_via(&test, &connected.account).as_deref(), Some("connect"));
}
