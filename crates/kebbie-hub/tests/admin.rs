#[allow(dead_code)]
mod common;


use common::*;
use kebbie_hub::store::Store;

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

fn user_version(path: &std::path::Path) -> i64 {
    rusqlite::Connection::open(path)
        .unwrap()
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap()
}

#[test]
fn a_fresh_database_starts_at_schema_16() {
    let data = tempfile::tempdir().unwrap();
    let path = data.path().join("hub.sqlite3");
    drop(Store::open(&path).unwrap());
    assert_eq!(user_version(&path), 16);
    drop(Store::open(&path).unwrap());
    assert_eq!(user_version(&path), 16);
}

fn columns(connection: &rusqlite::Connection, table: &str) -> Vec<String> {
    connection
        .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn a_schema_14_database_drops_its_device_tables_and_keeps_the_rest() {
    let data = tempfile::tempdir().unwrap();
    let path = data.path().join("hub.sqlite3");
    drop(Store::open(&path).unwrap());
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "DROP TABLE commands;
             DROP TABLE results;
             DROP TABLE pairings;
             DROP TABLE events;
             DROP TABLE devices;
             CREATE TABLE devices (id TEXT PRIMARY KEY, token_hash BLOB NOT NULL, account_id TEXT);
             CREATE TABLE admissions (machine_id TEXT NOT NULL, device_id TEXT NOT NULL);
             CREATE TABLE push_subscriptions (device_id TEXT PRIMARY KEY, endpoint TEXT NOT NULL);
             CREATE TABLE notices (machine_id TEXT NOT NULL, notice_id TEXT NOT NULL);
             CREATE TABLE device_lists (account_id TEXT PRIMARY KEY, counter INTEGER NOT NULL);
             CREATE TABLE commands (device_id TEXT NOT NULL, command_id TEXT NOT NULL);
             CREATE TABLE results (device_id TEXT NOT NULL, seq INTEGER NOT NULL);
             CREATE TABLE pairings (id TEXT PRIMARY KEY, device_id TEXT, code_hash BLOB);
             CREATE TABLE events (id INTEGER PRIMARY KEY, kind TEXT NOT NULL, device_id TEXT);
             INSERT INTO devices VALUES ('dev_old', x'01', NULL);
             INSERT INTO commands VALUES ('dev_old', 'cmd_old');
             INSERT INTO accounts (id, handle, user_handle, created_at) VALUES ('acc_kept', 'alice', x'00112233445566778899aabbccddeeff', 1);
             PRAGMA user_version = 14;",
        )
        .unwrap();
    let store = Store::open(&path).unwrap();
    assert_eq!(store.account("acc_kept").unwrap().unwrap().handle, "alice");
    drop(store);
    assert_eq!(user_version(&path), 16);
    let connection = rusqlite::Connection::open(&path).unwrap();
    let devices = columns(&connection, "devices");
    assert!(devices.contains(&"cert".to_string()) && !devices.contains(&"token_hash".to_string()), "{devices:?}");
    let old_devices: i64 = connection.query_row("SELECT count(*) FROM devices", [], |row| row.get(0)).unwrap();
    assert_eq!(old_devices, 0);
    for gone in ["admissions", "push_subscriptions", "notices", "device_lists"] {
        let tables: i64 = connection
            .query_row("SELECT count(*) FROM sqlite_master WHERE name = ?1", [gone], |row| row.get(0))
            .unwrap();
        assert_eq!(tables, 0, "{gone}");
    }
    assert!(columns(&connection, "commands").contains(&"account_id".to_string()));
    assert!(columns(&connection, "results").contains(&"account_id".to_string()));
    assert!(columns(&connection, "pairings").contains(&"account_nonce".to_string()));
    for table in ["results", "pairings", "events"] {
        assert!(!columns(&connection, table).iter().any(|column| column.starts_with("device")), "{table}");
    }
}

#[test]
fn a_schema_15_database_drops_its_account_signed_commands_and_keeps_the_rest() {
    let data = tempfile::tempdir().unwrap();
    let path = data.path().join("hub.sqlite3");
    drop(Store::open(&path).unwrap());
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "PRAGMA foreign_keys = OFF;
             DROP TABLE commands;
             DROP TABLE devices;
             CREATE TABLE commands (account_id TEXT NOT NULL, command_id TEXT NOT NULL, machine_id TEXT NOT NULL, kind TEXT NOT NULL, digest BLOB NOT NULL, seq INTEGER NOT NULL, queued_at INTEGER NOT NULL, delivered_at INTEGER);
             INSERT INTO accounts (id, handle, user_handle, created_at) VALUES ('acc_kept', 'alice', x'00112233445566778899aabbccddeeff', 1);
             INSERT INTO commands VALUES ('acc_kept', 'cmd_old', 'mch_old', 'answer', x'01', 1, 1, NULL);
             INSERT INTO results (account_id, machine_id, command_id, digest, envelope, stored_at, expires_at) VALUES ('acc_kept', 'mch_old', 'cmd_old', x'01', '{}', 1, 9999999999);
             INSERT INTO queue VALUES ('mch_old', 1, '{}', 1, 9999999999);
             PRAGMA user_version = 15;",
        )
        .unwrap();
    let store = Store::open(&path).unwrap();
    assert_eq!(store.account("acc_kept").unwrap().unwrap().handle, "alice");
    drop(store);
    assert_eq!(user_version(&path), 16);
    let connection = rusqlite::Connection::open(&path).unwrap();
    for column in ["device_id", "envelope"] {
        assert!(columns(&connection, "commands").contains(&column.to_string()), "{column}");
    }
    assert!(columns(&connection, "devices").contains(&"signing_key".to_string()));
    for table in ["commands", "results", "queue"] {
        let left: i64 = connection.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0)).unwrap();
        assert_eq!(left, 0, "{table}");
    }
}

#[test]
fn a_schema_13_database_opens_as_16_and_keeps_what_an_embedding_binary_added() {
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
    assert_eq!(user_version(&path), 16);
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
fn a_database_from_before_schema_13_or_after_16_is_refused() {
    for version in [4, 12, 17] {
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

