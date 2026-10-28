#[allow(dead_code)]
mod common;

use std::time::Duration;

use common::*;
use kebbie_hub::store::Store;
use kebbie_hub_fakes::PairError;
use tokio::sync::oneshot;

const WEB_ADMIN: [&str; 11] = [
    "/v1/a/admin/overview",
    "/v1/a/admin/accounts",
    "/v1/a/admin/accounts/acc_x/deactivate",
    "/v1/a/admin/accounts/acc_x/activate",
    "/v1/a/admin/invites",
    "/v1/a/admin/machines",
    "/v1/a/admin/machines/mch_x",
    "/v1/a/admin/reports",
    "/v1/a/admin/reports/1/ack",
    "/v1/a/admin/reports/keys/AwMD/block",
    "/v1/a/admin/signals",
];

#[tokio::test]
async fn the_hub_has_no_web_admin_for_its_owner() {
    let test = start_hub(|_| {}).await;
    let mut owner = owner(&test).await;
    for path in WEB_ADMIN {
        let read = owner.get::<serde_json::Value>(path).await;
        assert_eq!(hub_code(read.unwrap_err()).0, 404, "GET {path}");
        let act = owner.post::<_, serde_json::Value>(path, &()).await;
        assert_eq!(hub_code(act.unwrap_err()).0, 404, "POST {path}");
    }
    let removed = owner.delete("/v1/a/admin/machines/mch_x").await;
    assert_eq!(hub_code(removed.unwrap_err()).0, 404);
    let page = reqwest::get(format!("{}/admin", test.url)).await.unwrap();
    assert_eq!(page.status(), 404, "no /admin page");
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
    assert!(first.account().await.unwrap().owner);

    let replaced = admin(&[&second_id, "--replace"]);
    assert!(replaced.status.success(), "{replaced:?}");
    assert_eq!(
        String::from_utf8_lossy(&replaced.stdout).trim(),
        format!("{second_id} is the owner; {first_id} is a member now")
    );
    assert!(second.account().await.unwrap().owner);
    assert!(!first.account().await.unwrap().owner);
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

