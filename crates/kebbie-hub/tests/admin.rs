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

fn user_version(path: &std::path::Path) -> i64 {
    rusqlite::Connection::open(path)
        .unwrap()
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap()
}

#[test]
fn a_fresh_database_starts_at_schema_14() {
    let data = tempfile::tempdir().unwrap();
    let path = data.path().join("hub.sqlite3");
    drop(Store::open(&path).unwrap());
    assert_eq!(user_version(&path), 14);
    drop(Store::open(&path).unwrap());
    assert_eq!(user_version(&path), 14);
}

#[test]
fn a_schema_13_database_opens_as_14_and_keeps_what_an_embedding_binary_added() {
    let data = tempfile::tempdir().unwrap();
    let path = data.path().join("hub.sqlite3");
    drop(Store::open(&path).unwrap());
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "ALTER TABLE accounts ADD COLUMN tier TEXT NOT NULL DEFAULT 'basic';
             CREATE TABLE extension_rows (id INTEGER PRIMARY KEY, note TEXT NOT NULL);
             INSERT INTO extension_rows (note) VALUES ('kept');
             INSERT INTO accounts (id, handle, user_handle, created_at, tier) VALUES ('acc_kept', 'alice', x'00112233445566778899aabbccddeeff', 1, 'gold');
             INSERT INTO invites (code_hash, expires_at) VALUES (x'02', 9999999999);
             PRAGMA user_version = 13;",
        )
        .unwrap();
    let mut store = Store::open(&path).unwrap();
    assert_eq!(store.account("acc_kept").unwrap().unwrap().handle, "alice");
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
    assert!(matches!(outcome, kebbie_hub::store::SignUpOutcome::Created(_)));
    drop(store);
    assert_eq!(user_version(&path), 14);
    let connection = rusqlite::Connection::open(&path).unwrap();
    let tiers: Vec<String> = connection
        .prepare("SELECT tier FROM accounts ORDER BY created_at")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(tiers, ["gold", "basic"], "the added column keeps its values and default");
    let note: String = connection
        .query_row("SELECT note FROM extension_rows", [], |row| row.get(0))
        .unwrap();
    assert_eq!(note, "kept");
}

#[test]
fn a_database_from_before_schema_13_or_after_14_is_refused() {
    for version in [4, 12, 15] {
        let data = tempfile::tempdir().unwrap();
        let path = data.path().join("hub.sqlite3");
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch(&format!(
                "CREATE TABLE machines (id TEXT PRIMARY KEY); PRAGMA user_version = {version};"
            ))
            .unwrap();
        let refused = Store::open(&path).err().expect("refused").to_string();
        assert!(
            refused.contains(&format!("schema {version}")) && refused.contains("empty data directory"),
            "{refused}"
        );
        assert_eq!(user_version(&path), version, "nothing was changed");
    }
}

#[tokio::test]
async fn the_admin_cli_deactivates_and_activates_an_account_but_never_the_owner() {
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
    let mut first = owner(&test).await;
    let mut person = member(&test).await;
    let owner_id = first.account().await.unwrap().account;
    let account = person.account().await.unwrap().account;

    let deactivated = cli(&["deactivate", &account]);
    assert!(deactivated.status.success(), "{deactivated:?}");
    assert_eq!(hub_code(person.account().await.unwrap_err()).0, 401);
    assert_eq!(hub_code(person.log_in().await.unwrap_err()).0, 401);
    let listed = String::from_utf8_lossy(&cli(&["accounts"]).stdout).to_string();
    assert!(listed.contains(&format!("{account} member deactivated")), "{listed}");

    let refused = cli(&["deactivate", &owner_id]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("owner"), "{refused:?}");
    assert!(first.account().await.is_ok());
    assert_eq!(cli(&["activate", "acc_nobody"]).status.code(), Some(1));
    assert_eq!(cli(&["activate", "nobody"]).status.code(), Some(1));

    let activated = cli(&["activate", &account]);
    assert!(activated.status.success(), "{activated:?}");
    person.log_in().await.unwrap();
    let logged: Vec<(String, Option<String>)> = test
        .hub
        .store()
        .events()
        .unwrap()
        .into_iter()
        .filter(|(kind, _)| kind == "account.deactivated" || kind == "account.activated")
        .collect();
    assert_eq!(
        logged,
        vec![
            ("account.deactivated".to_string(), Some(format!("{account} by=cli"))),
            ("account.activated".to_string(), Some(format!("{account} by=cli"))),
        ]
    );
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

