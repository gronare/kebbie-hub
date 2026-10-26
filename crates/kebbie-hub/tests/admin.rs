#[allow(dead_code)]
mod common;

use std::time::Duration;

use common::*;
use kebbie_hub::store::{KeyStatus, NewReport, Store};
use kebbie_hub_api::{
    AdminAccounts, AdminMachines, AdminOverview, AdminReports, AdminSignals, InviteLink,
};
use kebbie_hub_fakes::PairError;
use kebbie_hub_protocol::BugReport;
use tokio::sync::oneshot;

const OWNER_READS: [&str; 5] = [
    "/v1/a/admin/overview",
    "/v1/a/admin/accounts",
    "/v1/a/admin/machines",
    "/v1/a/admin/reports",
    "/v1/a/admin/signals",
];

#[tokio::test]
async fn the_admin_endpoints_answer_only_the_owner() {
    let test = start_hub(|_| {}).await;
    let mut owner = owner(&test).await;
    let mut member = member(&test).await;
    let mut anonymous = browser(&test);
    for path in OWNER_READS {
        let refused = member.get::<serde_json::Value>(path).await;
        assert_eq!(
            hub_code(refused.unwrap_err()),
            (403, "not-owner".to_string()),
            "{path}"
        );
        let refused = anonymous.get::<serde_json::Value>(path).await;
        assert_eq!(hub_code(refused.unwrap_err()).0, 401, "{path}");
        owner.get::<serde_json::Value>(path).await.unwrap();
    }
    for path in [
        "/v1/a/admin/invites",
        "/v1/a/admin/accounts/acc_x/deactivate",
        "/v1/a/admin/reports/1/ack",
    ] {
        let refused = member.post::<_, serde_json::Value>(path, &()).await;
        assert_eq!(hub_code(refused.unwrap_err()).0, 403, "{path}");
    }
    assert_eq!(
        hub_code(
            member
                .delete("/v1/a/admin/machines/mch_x")
                .await
                .unwrap_err()
        )
        .0,
        403
    );
    owner.origin = "https://evil.example".to_string();
    let cross = owner
        .post::<_, serde_json::Value>("/v1/a/admin/invites", &())
        .await;
    assert_eq!(
        hub_code(cross.unwrap_err()),
        (403, "cross-origin".to_string())
    );
}

#[tokio::test]
async fn the_owner_sees_accounts_with_their_machines_devices_and_plans() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let mut owner = owner(&test).await;
    let mut person = member(&test).await;
    let mut machine = connected(&test, &mut person).await;
    person.ensure_device().await.unwrap();
    machine.post_board(board(MARKER)).await.unwrap();
    let overview: AdminOverview = owner.get("/v1/a/admin/overview").await.unwrap();
    assert_eq!(
        (overview.accounts, overview.machines, overview.devices),
        (2, 1, 1)
    );
    let listed: AdminAccounts = owner.get("/v1/a/admin/accounts").await.unwrap();
    let id = person.account().await.unwrap().account;
    let row = listed
        .accounts
        .iter()
        .find(|row| row.account == id)
        .unwrap();
    assert_eq!((row.machines, row.devices, row.owner), (1, 1, false));
    assert!(row.last_seen.is_some());
    assert!(row.plan.entitled);
    assert!(listed.accounts.iter().any(|row| row.owner));
    let machines: AdminMachines = owner.get("/v1/a/admin/machines").await.unwrap();
    assert_eq!(
        machines.machines[0].owner.as_deref(),
        Some(id.as_str())
    );
}

#[tokio::test]
async fn a_deactivated_account_loses_its_session_login_devices_and_machines() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let mut owner = owner(&test).await;
    let mut person = member(&test).await;
    let mut machine = connected(&test, &mut person).await;
    let phone = person.ensure_device().await.unwrap().clone();
    let account = person.account().await.unwrap().account;
    let mut feed = EventStream::open(&test.url, phone.token().unwrap()).await;
    let mut account_feed = EventStream::of_account(&person).await;
    owner
        .post::<_, Option<()>>(&format!("/v1/a/admin/accounts/{account}/deactivate"), &())
        .await
        .unwrap();
    assert_eq!(feed.next(Duration::from_secs(5)).await, Next::Ended);
    assert_eq!(account_feed.next(Duration::from_secs(5)).await, Next::Ended);
    assert_eq!(hub_code(person.account().await.unwrap_err()).0, 401);
    assert_eq!(hub_code(person.log_in().await.unwrap_err()).0, 401);
    assert!(person.boards().await.is_err());
    assert!(
        phone
            .client()
            .get::<serde_json::Value>("/v1/d/results", &[])
            .await
            .is_err()
    );
    assert!(machine.post_board(board(MARKER)).await.is_err());
    let own = owner.account().await.unwrap().account;
    let refused = owner
        .post::<_, Option<()>>(&format!("/v1/a/admin/accounts/{own}/deactivate"), &())
        .await;
    assert_eq!(
        hub_code(refused.unwrap_err()),
        (409, "owner-stays".to_string())
    );

    owner
        .post::<_, Option<()>>(&format!("/v1/a/admin/accounts/{account}/activate"), &())
        .await
        .unwrap();
    person.log_in().await.unwrap();
    machine.post_board(board(MARKER)).await.unwrap();
}

#[tokio::test]
async fn the_owner_removes_a_machine_and_its_token_stops() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let mut owner = owner(&test).await;
    let mut person = member(&test).await;
    let mut machine = connected(&test, &mut person).await;
    owner
        .delete(&format!("/v1/a/admin/machines/{}", machine.machine))
        .await
        .unwrap();
    assert_eq!(
        machine
            .post_board(board(MARKER))
            .await
            .err()
            .map(|e| e.to_string().contains("401")),
        Some(true)
    );
    let gone = owner
        .delete(&format!("/v1/a/admin/machines/{}", machine.machine))
        .await;
    assert_eq!(hub_code(gone.unwrap_err()).0, 404);
}

fn report(title: &str) -> BugReport {
    BugReport {
        title: title.to_string(),
        text: "the text".to_string(),
        version: "0.27.0".to_string(),
        os: "macos aarch64".to_string(),
    }
}

#[tokio::test]
async fn the_owner_decides_on_reports_from_the_web() {
    let test = start_hub(|_| {}).await;
    let mut owner = owner(&test).await;
    {
        let mut store = Store::open(&test.hub.config().database()).unwrap();
        store
            .store_report(&NewReport {
                key: &[3; 32],
                report: &report("waiting"),
                now: kebbie_hub::now(),
                max_quarantined: 10,
                max_quarantined_per_key: 10,
            })
            .unwrap();
    }
    let waiting: AdminReports = owner
        .get("/v1/a/admin/reports?quarantine=true")
        .await
        .unwrap();
    assert_eq!(waiting.reports.len(), 1);
    let key = serde_json::to_value(waiting.reports[0].key).unwrap();
    let key = key.as_str().unwrap();
    owner
        .post::<_, Option<()>>(&format!("/v1/a/admin/reports/keys/{key}/allow"), &())
        .await
        .unwrap();
    let allowed: AdminReports = owner.get("/v1/a/admin/reports").await.unwrap();
    assert_eq!(allowed.reports[0].status, "allowed");
    let overview: AdminOverview = owner.get("/v1/a/admin/overview").await.unwrap();
    assert_eq!((overview.reports, overview.quarantined), (1, 0));
    owner
        .post::<_, Option<()>>(
            &format!("/v1/a/admin/reports/{}/ack", allowed.reports[0].id),
            &(),
        )
        .await
        .unwrap();
    let handled: AdminReports = owner.get("/v1/a/admin/reports").await.unwrap();
    assert!(handled.reports.is_empty());
    owner
        .post::<_, Option<()>>(&format!("/v1/a/admin/reports/keys/{key}/block"), &())
        .await
        .unwrap();
    assert_eq!(
        test.hub
            .store()
            .reports(KeyStatus::Blocked, 0, false, 10)
            .unwrap()
            .len(),
        0
    );
    let signals: AdminSignals = owner.get("/v1/a/admin/signals?days=30").await.unwrap();
    assert!(signals.signals.is_empty());
}

#[tokio::test]
async fn another_account_cannot_pair_a_device_with_my_machine() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let mut person = member(&test).await;
    let mut machine = connected(&test, &mut person).await;
    let mut stranger = member(&test).await;
    let (code_out, code_in) = oneshot::channel::<String>();
    let machine_side = machine.pair(
        Duration::from_secs(2),
        move |code| {
            let _ = code_out.send(code.to_string());
        },
        async |_: String| true,
    );
    let stranger_side = async {
        let code = code_in.await.unwrap();
        let device = stranger.ensure_device().await.unwrap();
        device
            .pair(&code, Duration::from_secs(2), async |_: String| true)
            .await
    };
    let (admitted, paired) = tokio::join!(machine_side, stranger_side);
    assert!(admitted.is_err());
    match paired {
        Err(PairError::Hub(error)) => {
            assert_eq!(hub_code(error), (403, "not-your-machine".to_string()))
        }
        other => panic!("a stranger paired: {other:?}"),
    }
}

#[test]
fn a_schema_6_database_is_upgraded_in_place_and_keeps_its_data() {
    let data = tempfile::tempdir().unwrap();
    let path = data.path().join("hub.sqlite3");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(include_str!("fixtures/schema-6.sql"))
        .unwrap();
    connection
        .execute_batch(
            "INSERT INTO accounts (id, handle, user_handle, created_at) VALUES ('acc_later', 'ada', x'ffeeddccbbaa99887766554433221100', 5);
             INSERT INTO accounts (id, handle, user_handle, created_at) VALUES ('acc_kept', 'carl', x'00112233445566778899aabbccddeeff', 1);
             INSERT INTO machines (id, name, signing_key, encryption_key, token_hash, joined_at)
                 VALUES ('mch_00000000000000000000000000', 'mac', zeroblob(32), zeroblob(32), x'01', 1);
             INSERT INTO invites (code_hash, expires_at) VALUES (x'02', 9999999999);
             INSERT INTO admin_tokens (token_hash, name, created_at) VALUES (x'03', 'laptop', 1);
             INSERT INTO passkeys (credential_id, account_id, passkey, created_at) VALUES (x'0a', 'acc_kept', '{}', 1);
             INSERT INTO passkeys (credential_id, account_id, passkey, created_at) VALUES (x'0b', 'acc_kept', '{}', 2);",
        )
        .unwrap();
    drop(connection);
    let store = Store::open(&path).unwrap();
    let account = store.account("acc_kept").unwrap().unwrap();
    assert_eq!(
        (account.handle.as_str(), account.owner),
        ("carl", true),
        "the oldest account becomes the owner"
    );
    assert!(!store.account("acc_later").unwrap().unwrap().owner);
    assert_eq!(
        account.plan,
        kebbie_hub_api::PlanStatus::Active,
        "an account from before plans keeps working"
    );
    assert!(store.ticket(b"\x02", 1).unwrap().is_some());
    let salts: Vec<[u8; 32]> = store
        .passkeys("acc_kept")
        .unwrap()
        .iter()
        .map(|passkey| passkey.prf_salt)
        .collect();
    assert_eq!(salts.len(), 2);
    assert!(salts[0] != [0; 32] && salts[0] != salts[1], "every older passkey gets a salt of its own");
    assert_eq!(store.account_key("acc_kept").unwrap(), None);
    let mut store = store;
    let outcome = store
        .sign_up(&kebbie_hub::store::SignUp {
            invite_hash: b"\x02",
            connect: false,
            account: "acc_new",
            handle: "newcomer",
            user_handle: [9; 16],
            credential_id: b"cred",
            name: None,
            passkey: "{}",
            prf_salt: [5; 32],
            session_hash: b"session",
            session_expires_at: u64::MAX >> 2,
            now: 2,
        })
        .unwrap();
    assert!(matches!(
        outcome,
        kebbie_hub::store::SignUpOutcome::Created(_)
    ));
    assert_eq!(
        store.account("acc_new").unwrap().unwrap().plan,
        kebbie_hub_api::PlanStatus::Inactive
    );
    assert_eq!(store.machines().unwrap()[0].name, "mac");
    let tokens = store.admin_tokens().unwrap();
    assert_eq!(tokens[0].scope, kebbie_hub::store::AdminScope::Reports);
    drop(store);
    let version: i64 = rusqlite::Connection::open(&path)
        .unwrap()
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 10);
    assert_eq!(store_created_via(&path, "acc_kept"), None, "an account from before keeps no origin");
    assert_eq!(store_created_via(&path, "acc_new").as_deref(), Some("invite"));
    assert!(!store_account_is_owner(&path, "acc_new"));
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch("UPDATE accounts SET role = 'member'")
        .unwrap();
    let reopened = Store::open(&path).unwrap();
    assert_eq!(
        reopened.account("acc_new").unwrap().unwrap().plan,
        kebbie_hub_api::PlanStatus::Inactive,
        "a later start does not activate new accounts"
    );
    for account in ["acc_kept", "acc_later", "acc_new"] {
        assert!(
            !reopened.account(account).unwrap().unwrap().owner,
            "a later start makes no owner"
        );
    }
}

fn store_account_is_owner(path: &std::path::Path, account: &str) -> bool {
    Store::open(path)
        .unwrap()
        .account(account)
        .unwrap()
        .unwrap()
        .owner
}

#[tokio::test]
async fn the_admin_cli_makes_an_account_the_one_owner() {
    let test = start_hub(|_| {}).await;
    let cli = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_kebbie-hub"))
            .arg("--data")
            .arg(test.data.path())
            .arg("admin")
            .args(args)
            .output()
            .unwrap()
    };
    let admin = |args: &[&str]| cli(&[&["owner"], args].concat());
    let mut first = owner(&test).await;
    let mut second = member(&test).await;
    let first_id = first.account().await.unwrap().account;
    let second_id = second.account().await.unwrap().account;
    let listed = String::from_utf8_lossy(&cli(&["accounts"]).stdout).to_string();
    assert!(listed.contains(&format!("{first_id} owner")), "{listed}");
    assert!(listed.contains(&format!("{second_id} member")), "{listed}");
    assert!(!listed.contains(&second.handle), "{listed}");
    let by_handle = admin(&[&second.handle]);
    assert_eq!(by_handle.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&by_handle.stderr).contains("not an account id"),
        "{by_handle:?}"
    );
    let refused = admin(&[&second_id]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("--replace"),
        "{refused:?}"
    );
    first
        .get::<AdminOverview>("/v1/a/admin/overview")
        .await
        .unwrap();

    let replaced = admin(&[&second_id, "--replace"]);
    assert!(replaced.status.success(), "{replaced:?}");
    assert_eq!(
        String::from_utf8_lossy(&replaced.stdout).trim(),
        format!("{second_id} is the owner; {first_id} is a member now")
    );
    second
        .get::<AdminOverview>("/v1/a/admin/overview")
        .await
        .unwrap();
    assert_eq!(
        hub_code(
            first
                .get::<AdminOverview>("/v1/a/admin/overview")
                .await
                .unwrap_err()
        ),
        (403, "not-owner".to_string())
    );
    let again = admin(&[&second_id]);
    assert!(again.status.success());
    assert!(String::from_utf8_lossy(&again.stdout).contains("already"));
    let unknown = admin(&["acc_nobody"]);
    assert_eq!(unknown.status.code(), Some(1));

    let store = Store::open(&test.data.path().join("hub.sqlite3")).unwrap();
    let owners = store
        .admin_accounts()
        .unwrap()
        .into_iter()
        .filter(|row| row.account.owner)
        .count();
    assert_eq!(owners, 1);
    let logged: i64 = rusqlite::Connection::open(test.data.path().join("hub.sqlite3"))
        .unwrap()
        .query_row(
            "SELECT count(*) FROM events WHERE kind = 'account.owner'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(logged, 1);
}

fn store_created_via(path: &std::path::Path, account: &str) -> Option<String> {
    Store::open(path).unwrap().account(account).unwrap().unwrap().created_via
}

#[test]
fn an_owner_made_without_another_owner_needs_no_replace() {
    let data = tempfile::tempdir().unwrap();
    let path = data.path().join("hub.sqlite3");
    let store = Store::open(&path).unwrap();
    drop(store);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "INSERT INTO accounts (id, handle, user_handle, created_at) VALUES ('acc_a', 'ada', x'00112233445566778899aabbccddeeff', 1);
             INSERT INTO accounts (id, handle, user_handle, created_at, deactivated_at) VALUES ('acc_b', 'bo', x'ffeeddccbbaa99887766554433221100', 2, 3);",
        )
        .unwrap();
    let store = Store::open(&path).unwrap();
    assert_eq!(
        store.make_owner("acc_b", false, 4).unwrap(),
        kebbie_hub::store::MadeOwner::Deactivated
    );
    assert_eq!(
        store.make_owner("acc_a", false, 4).unwrap(),
        kebbie_hub::store::MadeOwner::Made { replaced: vec![] }
    );
    assert!(store.account("acc_a").unwrap().unwrap().owner);
}

#[tokio::test]
async fn destructive_owner_actions_need_a_fresh_passkey_session() {
    let test = start_hub(|config| {
        config.fresh_session_ttl = 1;
        config.connect_interval = 0;
    })
    .await;
    let mut owner = owner(&test).await;
    let mut person = member(&test).await;
    let machine = connected(&test, &mut person).await;
    let account = person.account().await.unwrap().account;
    tokio::time::sleep(Duration::from_millis(2100)).await;
    let key = "AwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwM";
    let stale = [
        owner
            .post::<_, Option<()>>(&format!("/v1/a/admin/accounts/{account}/deactivate"), &())
            .await,
        owner
            .delete(&format!("/v1/a/admin/machines/{}", machine.machine))
            .await
            .map(|()| None),
        owner
            .post::<_, Option<()>>(&format!("/v1/a/admin/reports/keys/{key}/block"), &())
            .await,
    ];
    for refused in stale {
        assert_eq!(
            hub_code(refused.unwrap_err()),
            (401, "reauth-required".to_string())
        );
    }
    for path in OWNER_READS {
        owner.get::<serde_json::Value>(path).await.unwrap();
    }
    owner
        .post::<_, InviteLink>("/v1/a/admin/invites", &())
        .await
        .unwrap();
    owner
        .post::<_, Option<()>>(&format!("/v1/a/admin/reports/keys/{key}/allow"), &())
        .await
        .unwrap();
    owner.log_in().await.unwrap();
    owner
        .post::<_, Option<()>>(&format!("/v1/a/admin/reports/keys/{key}/block"), &())
        .await
        .unwrap();
    owner
        .post::<_, Option<()>>(&format!("/v1/a/admin/accounts/{account}/deactivate"), &())
        .await
        .unwrap();
    owner
        .delete(&format!("/v1/a/admin/machines/{}", machine.machine))
        .await
        .unwrap();
}
