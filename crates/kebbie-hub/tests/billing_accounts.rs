#[allow(dead_code)]
mod common;

use std::time::Duration;

use common::*;
use kebbie_hub::store::AdminScope;
use kebbie_hub_api::{BillingAccountDetail, BillingAccounts, PlanStatus, PlanUpdate};
use kebbie_hub_fakes::HubClient;
use serde_json::Value;

const SOON: Duration = Duration::from_secs(5);

fn admin(test: &TestHub, scope: AdminScope) -> HubClient {
    let token = test
        .hub
        .mint_admin_token(&format!("{scope:?}-{}", fresh_name()), scope)
        .unwrap()
        .unwrap();
    HubClient::new(&test.url, Some(&token))
}

async fn page(billing: &HubClient, after: Option<&str>, limit: Option<u64>) -> BillingAccounts {
    let mut query = Vec::new();
    if let Some(after) = after {
        query.push(("after", after.to_string()));
    }
    if let Some(limit) = limit {
        query.push(("limit", limit.to_string()));
    }
    billing.get("/v1/admin/accounts", &query).await.unwrap()
}

fn database(test: &TestHub) -> rusqlite::Connection {
    rusqlite::Connection::open(test.hub.config().database()).unwrap()
}

#[tokio::test]
async fn the_account_list_pages_oldest_first_without_email_or_owner() {
    let test = start_hub(|_| {}).await;
    let mut owner = owner(&test).await;
    let mut people = Vec::new();
    for _ in 0..4 {
        people.push(member(&test).await);
    }
    let owner_id = owner.account().await.unwrap().account;
    database(&test)
        .execute(
            "UPDATE accounts SET email = 'kept@example.com', email_verified_at = 1 WHERE id = ?1",
            [&owner_id],
        )
        .unwrap();
    let billing = admin(&test, AdminScope::Billing);

    let all = page(&billing, None, None).await;
    assert_eq!(all.accounts.len(), 5);
    assert_eq!(all.next, None);
    let mut order: Vec<(u64, String)> = all
        .accounts
        .iter()
        .map(|row| (row.created_at, row.account.clone()))
        .collect();
    order.sort();
    assert_eq!(
        order.iter().map(|(_, id)| id.clone()).collect::<Vec<_>>(),
        all.accounts.iter().map(|row| row.account.clone()).collect::<Vec<_>>(),
        "oldest first"
    );

    let mut walked = Vec::new();
    let mut after = None;
    loop {
        let one = page(&billing, after.as_deref(), Some(2)).await;
        assert!(one.accounts.len() <= 2);
        walked.extend(one.accounts.iter().map(|row| row.account.clone()));
        match one.next {
            Some(next) => {
                assert_eq!(Some(&next), walked.last());
                after = Some(next);
            }
            None => break,
        }
    }
    assert_eq!(
        walked,
        all.accounts.iter().map(|row| row.account.clone()).collect::<Vec<_>>()
    );
    let exact = page(&billing, None, Some(5)).await;
    assert_eq!((exact.accounts.len(), exact.next), (5, None), "a full last page has no next");

    let raw: Value = billing.get("/v1/admin/accounts", &[]).await.unwrap();
    for row in raw["accounts"].as_array().unwrap() {
        assert!(row.get("email").is_none() && row.get("owner").is_none(), "{row}");
    }
    assert!(!raw.to_string().contains("kept@example.com"));

    for (limit, code) in [("0", "limit-out-of-range"), ("501", "limit-out-of-range")] {
        let refused = billing
            .get::<Value>("/v1/admin/accounts", &[("limit", limit.to_string())])
            .await;
        assert_eq!(hub_code(refused.unwrap_err()), (400, code.to_string()));
    }
    let unknown = billing
        .get::<Value>("/v1/admin/accounts", &[("after", "acc_nobody".to_string())])
        .await;
    assert_eq!(hub_code(unknown.unwrap_err()), (400, "unknown-after".to_string()));
}

#[tokio::test]
async fn a_listed_account_counts_its_machines_active_ones_and_its_github_login() {
    let test = start_hub(|config| {
        config.connect_interval = 0;
        config.plans = true;
    })
    .await;
    let mut person = member(&test).await;
    let account = person.account().await.unwrap().account;
    let billing = admin(&test, AdminScope::Billing);
    billing
        .put::<_, Value>(
            &format!("/v1/admin/accounts/{account}/plan"),
            &PlanUpdate {
                status: PlanStatus::Active,
                until: Some(4_000_000_000),
            },
        )
        .await
        .unwrap();
    let mut recent = connected(&test, &mut person).await;
    recent.post_board(board(MARKER)).await.unwrap();
    let idle = connected(&test, &mut person).await;
    let week_ago = kebbie_hub::now() - 8 * 86_400;
    database(&test)
        .execute(
            "UPDATE machines SET last_seen = ?2 WHERE id = ?1",
            rusqlite::params![idle.machine.as_str(), week_ago],
        )
        .unwrap();
    test.hub
        .store()
        .link_github(&account, 4242, "octo-cat")
        .unwrap();
    let listed = page(&billing, None, None).await;
    let row = listed
        .accounts
        .iter()
        .find(|row| row.account == account)
        .unwrap();
    assert_eq!(row.github.as_deref(), Some("octo-cat"));
    assert_eq!((row.machines, row.active_machines), (2, 1));
    assert!(row.last_seen.unwrap() > week_ago);
    assert_eq!(row.deactivated_at, None);
    assert_eq!(
        (row.plan.status, row.plan.until, row.plan.entitled),
        (PlanStatus::Active, Some(4_000_000_000), true)
    );
    let quiet = member(&test).await.account().await.unwrap().account;
    let listed = page(&billing, None, None).await;
    let fresh = listed.accounts.iter().find(|row| row.account == quiet).unwrap();
    assert_eq!(
        (fresh.github.as_deref(), fresh.machines, fresh.active_machines, fresh.last_seen),
        (None, 0, 0, None)
    );
}

#[tokio::test]
async fn one_account_shows_its_origin_passkeys_and_machines_but_no_board() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let mut person = member(&test).await;
    let account = person.account().await.unwrap().account;
    person.add_passkey().await.unwrap();
    let mut posting = connected(&test, &mut person).await;
    posting.post_board(board(MARKER)).await.unwrap();
    let silent = connected(&test, &mut person).await;
    let billing = admin(&test, AdminScope::Billing);

    let path = format!("/v1/admin/accounts/{account}");
    let detail: BillingAccountDetail = billing.get(&path, &[]).await.unwrap();
    assert_eq!(detail.account, account);
    assert_eq!(detail.created_via.as_deref(), Some("invite"));
    assert_eq!(detail.passkeys, 2);
    assert_eq!(detail.github, None);
    assert!(detail.last_seen.is_some());
    assert_eq!(detail.machines.len(), 2);
    let shown = |id: &kebbie_hub_protocol::Id| {
        detail
            .machines
            .iter()
            .find(|machine| &machine.machine == id)
            .unwrap()
            .clone()
    };
    let posted = shown(&posting.machine);
    assert_eq!(posted.name, "mac");
    assert!(posted.last_board_at.is_some() && posted.last_seen.is_some());
    assert!(posted.joined_at <= posted.last_board_at.unwrap());
    assert_eq!(shown(&silent.machine).last_board_at, None);

    let raw: Value = billing.get(&path, &[]).await.unwrap();
    assert!(raw.get("email").is_none() && raw.get("owner").is_none(), "{raw}");
    let text = raw.to_string();
    assert!(!text.contains(MARKER) && !text.contains("envelope"), "{text}");

    let owner_hub = start_hub(|_| {}).await;
    let mut made = owner(&owner_hub).await;
    let owner_id = made.account().await.unwrap().account;
    let owner_billing = admin(&owner_hub, AdminScope::Billing);
    let detail: BillingAccountDetail = owner_billing
        .get(&format!("/v1/admin/accounts/{owner_id}"), &[])
        .await
        .unwrap();
    assert_eq!(detail.created_via.as_deref(), Some("setup"));
    database(&owner_hub)
        .execute("UPDATE accounts SET created_via = NULL", [])
        .unwrap();
    let raw: Value = owner_billing
        .get(&format!("/v1/admin/accounts/{owner_id}"), &[])
        .await
        .unwrap();
    assert!(raw["created_via"].is_null() && raw["github"].is_null(), "{raw}");

    let missing = billing
        .get::<Value>("/v1/admin/accounts/acc_nobody", &[])
        .await;
    assert_eq!(hub_code(missing.unwrap_err()), (404, "not-found".to_string()));
}

#[tokio::test]
async fn the_account_endpoints_take_only_a_billing_token() {
    let test = start_hub(|_| {}).await;
    let mut person = member(&test).await;
    let account = person.account().await.unwrap().account;
    let reports = admin(&test, AdminScope::Reports);
    let anonymous = HubClient::new(&test.url, None);
    let reads = ["/v1/admin/accounts".to_string(), format!("/v1/admin/accounts/{account}")];
    for path in &reads {
        let refused = reports.get::<Value>(path, &[]).await;
        assert_eq!(hub_code(refused.unwrap_err()), (403, "wrong-scope".to_string()), "{path}");
        let refused = anonymous.get::<Value>(path, &[]).await;
        assert_eq!(hub_code(refused.unwrap_err()), (401, "unauthorized".to_string()), "{path}");
    }
    for action in ["deactivate", "activate"] {
        let path = format!("/v1/admin/accounts/{account}/{action}");
        let refused = reports.post_empty(&path, &()).await;
        assert_eq!(hub_code(refused.unwrap_err()), (403, "wrong-scope".to_string()), "{path}");
        let refused = anonymous.post_empty(&path, &()).await;
        assert_eq!(hub_code(refused.unwrap_err()).0, 401, "{path}");
    }
    let cookie = HubClient::new(&test.url, None)
        .with_header("cookie", person.cookie.as_deref().unwrap())
        .get::<Value>("/v1/admin/accounts", &[])
        .await;
    assert_eq!(hub_code(cookie.unwrap_err()).0, 401);
    assert!(person.account().await.is_ok(), "nothing was deactivated");
}

#[tokio::test]
async fn the_billing_token_deactivates_and_activates_as_the_owner_does() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let mut owner = owner(&test).await;
    let mut person = member(&test).await;
    let mut machine = connected(&test, &mut person).await;
    let phone = person.ensure_device().await.unwrap().clone();
    let account = person.account().await.unwrap().account;
    let mut feed = EventStream::open(&test.url, phone.token().unwrap()).await;
    let mut account_feed = EventStream::of_account(&person).await;
    let billing = admin(&test, AdminScope::Billing);

    billing
        .post_empty(&format!("/v1/admin/accounts/{account}/deactivate"), &())
        .await
        .unwrap();
    assert_eq!(feed.next(SOON).await, Next::Ended, "the device's feed ends");
    assert_eq!(account_feed.next(SOON).await, Next::Ended, "the account's feed ends");
    assert_eq!(hub_code(person.account().await.unwrap_err()).0, 401);
    assert_eq!(hub_code(person.log_in().await.unwrap_err()).0, 401);
    assert!(
        phone
            .client()
            .get::<Value>("/v1/d/results", &[])
            .await
            .is_err()
    );
    assert!(machine.post_board(board(MARKER)).await.is_err());
    let listed = page(&billing, None, None).await;
    let row = listed.accounts.iter().find(|row| row.account == account).unwrap();
    assert!(row.deactivated_at.is_some());

    let own = owner.account().await.unwrap().account;
    for action in ["deactivate", "activate"] {
        let refused = billing
            .post_empty(&format!("/v1/admin/accounts/{own}/{action}"), &())
            .await;
        assert_eq!(hub_code(refused.unwrap_err()), (409, "owner-stays".to_string()));
        let missing = billing
            .post_empty(&format!("/v1/admin/accounts/acc_nobody/{action}"), &())
            .await;
        assert_eq!(hub_code(missing.unwrap_err()), (404, "not-found".to_string()));
    }
    assert!(owner.account().await.is_ok());

    billing
        .post_empty(&format!("/v1/admin/accounts/{account}/activate"), &())
        .await
        .unwrap();
    person.log_in().await.unwrap();
    machine.post_board(board(MARKER)).await.unwrap();
    let logged: Vec<String> = test
        .hub
        .store()
        .events()
        .unwrap()
        .into_iter()
        .filter(|(kind, _)| kind.starts_with("account.deactivated") || kind.starts_with("account.activated"))
        .filter_map(|(_, detail)| detail)
        .collect();
    assert_eq!(
        logged,
        vec![
            format!("{account} by=billing-token"),
            format!("{account} by=billing-token")
        ]
    );
}
