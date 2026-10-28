#[allow(dead_code)]
mod common;

use common::*;
use kebbie_hub::store::AdminScope;
use kebbie_hub_api::{AccountPlan, PlanStatus, PlanUpdate, PlanView};
use kebbie_hub_fakes::machine::{AdmittedDevice, connect};
use kebbie_hub_fakes::HubClient;
use kebbie_hub_protocol::{Event, Id, Notice, Prefix, Role, SecretKeys, Urgency};

const BILLING: &str = "https://billing.example.com/start";

fn on(config: &mut kebbie_hub::Config) {
    config.plans = true;
    config.billing_url = Some(BILLING.to_string());
    config.connect_interval = 0;
}

fn admin(test: &TestHub, scope: AdminScope) -> HubClient {
    let token = test
        .hub
        .mint_admin_token(&format!("{scope:?}-{}", fresh_name()), scope)
        .unwrap()
        .unwrap();
    HubClient::new(&test.url, Some(&token))
}

async fn set_plan(
    test: &TestHub,
    account: &str,
    status: PlanStatus,
    until: Option<u64>,
) -> AccountPlan {
    admin(test, AdminScope::Billing)
        .put(
            &format!("/v1/admin/accounts/{account}/plan"),
            &PlanUpdate { status, until },
        )
        .await
        .unwrap()
}

fn notice() -> Notice {
    Notice {
        event: Event::NeedsYou,
        address: None,
        title: "kebbie".to_string(),
        body: "a task needs you".to_string(),
        question: None,
        options: Vec::new(),
        recommend: None,
        board_rev: 0,
        retract: None,
    }
}

#[tokio::test]
async fn with_plans_off_every_account_is_entitled() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let mut person = member(&test).await;
    let account = person.account().await.unwrap();
    assert_eq!(account.plan, PlanView::default());
    assert_eq!(account.billing_url, None);
    let mut machine = connected(&test, &mut person).await;
    machine.post_board(board(MARKER)).await.unwrap();
}

#[tokio::test]
async fn an_inactive_plan_shows_the_billing_link_and_gates_nothing() {
    let test = start_hub(on).await;
    let mut person = member(&test).await;
    let account = person.account().await.unwrap();
    assert_eq!(account.plan.status, PlanStatus::Inactive);
    assert!(!account.plan.entitled);
    assert_eq!(account.billing_url.as_deref(), Some(BILLING));
    let pending = connect(&test.url, "mac").await.unwrap();
    person
        .approve_connect(&pending.link(), false)
        .await
        .unwrap();
    pending.wait(WAIT).await.unwrap();
}

#[tokio::test]
async fn a_lapsed_plan_still_posts_and_reads_every_machine_call() {
    let test = start_hub(on).await;
    let mut person = member(&test).await;
    let account = person.account().await.unwrap().account;
    set_plan(&test, &account, PlanStatus::Active, None).await;
    let mut machine = connected_with_board(&test, &mut person).await;
    set_plan(
        &test,
        &account,
        PlanStatus::Active,
        Some(kebbie_hub::now() - 1),
    )
    .await;
    let page = person.account().await.unwrap();
    assert!(!page.plan.entitled);
    assert_eq!(page.billing_url.as_deref(), Some(BILLING));

    machine.post_board(board(MARKER)).await.unwrap();
    assert!(person.boards().await.unwrap().iter().any(|entry| entry.board.is_ok()), "the browser reads the board");
    machine.devices.push(AdmittedDevice {
        id: Id::new(Prefix::Device).unwrap(),
        keys: SecretKeys::generate().public(),
        role: Role::Control,
    });
    machine
        .notice(&notice(), Urgency::Normal, None)
        .await
        .unwrap();
    machine.poll_once(0).await.unwrap();
    machine
        .client()
        .post::<_, kebbie_hub_api::PairingOpened>("/v1/m/pairings", &())
        .await
        .unwrap();
}

#[tokio::test]
async fn the_owner_is_always_entitled_and_operator_machines_are_not_gated() {
    let test = start_hub(on).await;
    let mut owner = owner(&test).await;
    assert!(owner.account().await.unwrap().plan.entitled);
    let mut machine = connected(&test, &mut owner).await;
    machine.post_board(board(MARKER)).await.unwrap();
    let mut joined = joined_machine(&test).await;
    joined.post_board(board(MARKER)).await.unwrap();
}

#[tokio::test]
async fn the_plan_api_takes_only_a_billing_token() {
    let test = start_hub(on).await;
    let person = member(&test).await;
    let path = "/v1/admin/accounts/acc_x/plan";
    let update = PlanUpdate {
        status: PlanStatus::Active,
        until: None,
    };
    let anonymous = HubClient::new(&test.url, None)
        .put::<_, AccountPlan>(path, &update)
        .await;
    assert_eq!(
        hub_code(anonymous.unwrap_err()),
        (401, "unauthorized".to_string())
    );
    let reports = admin(&test, AdminScope::Reports)
        .put::<_, AccountPlan>(path, &update)
        .await;
    assert_eq!(
        hub_code(reports.unwrap_err()),
        (403, "wrong-scope".to_string())
    );
    let billing = admin(&test, AdminScope::Billing);
    let read_reports = billing
        .get::<kebbie_hub_api::ReportInbox>("/v1/admin/reports", &[])
        .await;
    assert_eq!(
        hub_code(read_reports.unwrap_err()),
        (403, "wrong-scope".to_string())
    );
    let unknown = billing.put::<_, AccountPlan>(path, &update).await;
    assert_eq!(hub_code(unknown.unwrap_err()).0, 404);
    let cookie = HubClient::new(&test.url, None)
        .with_header("cookie", person.cookie.as_deref().unwrap())
        .put::<_, AccountPlan>(path, &update)
        .await;
    assert_eq!(hub_code(cookie.unwrap_err()).0, 401);
}

#[tokio::test]
async fn the_billing_service_finds_an_account_carried_over_with_a_verified_email() {
    let test = start_hub(on).await;
    let mut person = member(&test).await;
    let account = person.account().await.unwrap();
    rusqlite::Connection::open(test.hub.config().database())
        .unwrap()
        .execute(
            "UPDATE accounts SET email = 'carl@example.com', email_verified_at = 1 WHERE id = ?1",
            [&account.account],
        )
        .unwrap();
    let billing = admin(&test, AdminScope::Billing);
    let found: AccountPlan = billing
        .get(
            "/v1/admin/accounts",
            &[("email", "Carl@Example.com".to_string())],
        )
        .await
        .unwrap();
    assert_eq!(found.account, account.account);
    assert_eq!(found.email.as_deref(), Some("carl@example.com"));
    assert_eq!(found.plan.status, PlanStatus::Inactive);
    let missing = billing
        .get::<AccountPlan>(
            "/v1/admin/accounts",
            &[("email", "nobody@example.com".to_string())],
        )
        .await;
    assert_eq!(hub_code(missing.unwrap_err()).0, 404);
    let set = set_plan(
        &test,
        &found.account,
        PlanStatus::Active,
        Some(4_000_000_000),
    )
    .await;
    assert_eq!(set.plan.until, Some(4_000_000_000));
    assert!(person.account().await.unwrap().plan.entitled);
}

#[test]
fn the_admin_cli_mints_a_billing_token() {
    let data = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_kebbie-hub"))
            .arg("--data")
            .arg(data.path())
            .arg("admin")
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap()
    };
    assert!(run(&["token", "--name", "billing", "--scope", "billing"]).starts_with("kha_"));
    assert!(run(&["token", "list"]).contains("billing  billing"));
}
