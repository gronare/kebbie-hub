#[allow(dead_code)]
mod common;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use kebbie_hub::RateLimit;
use kebbie_hub::store::{AdminScope, KeyStatus, Store};
use kebbie_hub_api::{
    Bytes32, Bytes64, ReportAccepted, ReportChallenge, ReportInbox, ReportUpload,
};
use kebbie_hub_protocol::{BugReport, InstallKey, MAX_TEXT_BYTES, meets, solve};
use reqwest::Response;
use serde_json::Value;

use common::{TestHub, body, refusal, rows, start_hub};

const DIFFICULTY: u8 = 8;

async fn hub() -> TestHub {
    start_hub(|config| config.report_difficulty = DIFFICULTY).await
}

fn report(title: &str) -> BugReport {
    BugReport {
        title: title.to_string(),
        text: format!("{title}: what happened and what was expected."),
        version: "0.26.0 (e58e84a)".to_string(),
        os: "macos aarch64".to_string(),
    }
}

fn install_key(seed: u8) -> InstallKey {
    InstallKey::from_bytes([seed; 32])
}

async fn challenge(test: &TestHub) -> ReportChallenge {
    body(
        reqwest::get(format!("{}/v1/reports/challenge", test.url))
            .await
            .unwrap(),
    )
    .await
}

fn signed(key: &InstallKey, report: BugReport, challenge: ReportChallenge) -> ReportUpload {
    let counter = solve(&challenge.nonce.0, &report.digest(), challenge.difficulty);
    ReportUpload {
        signature: Bytes64(key.sign(&challenge.nonce.0, &report)),
        key: Bytes32(key.public()),
        report,
        challenge,
        counter,
    }
}

async fn upload(test: &TestHub, key: &InstallKey, report: BugReport) -> ReportUpload {
    signed(key, report, challenge(test).await)
}

async fn post(test: &TestHub, upload: &ReportUpload) -> Response {
    reqwest::Client::new()
        .post(format!("{}/v1/reports", test.url))
        .json(upload)
        .send()
        .await
        .unwrap()
}

async fn accepted(test: &TestHub, upload: &ReportUpload) -> u64 {
    let response = post(test, upload).await;
    assert_eq!(response.status().as_u16(), 202);
    body::<ReportAccepted>(response).await.report
}

async fn admin_get(test: &TestHub, token: Option<&str>, after: u64) -> Response {
    let mut request =
        reqwest::Client::new().get(format!("{}/v1/admin/reports?after={after}", test.url));
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    request.send().await.unwrap()
}

async fn admin_ack(test: &TestHub, token: &str, id: u64) -> u16 {
    reqwest::Client::new()
        .post(format!("{}/v1/admin/reports/{id}/ack", test.url))
        .bearer_auth(token)
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

fn allow(test: &TestHub, key: &InstallKey) {
    test.hub
        .store()
        .set_key_status(&key.public(), KeyStatus::Allowed, kebbie_hub::now())
        .unwrap();
}

#[tokio::test]
async fn a_valid_report_waits_in_quarantine_until_its_key_is_allowed_then_reaches_the_admin_api_once_acked()
 {
    let test = hub().await;
    let token = test
        .hub
        .mint_admin_token("laptop", AdminScope::Reports)
        .unwrap()
        .unwrap();
    let key = install_key(1);
    let first = accepted(
        &test,
        &upload(&test, &key, report("push hid git's error")).await,
    )
    .await;
    let inbox: ReportInbox = body(admin_get(&test, Some(&token), 0).await).await;
    assert!(
        inbox.reports.is_empty(),
        "an unknown key's report is not handed out"
    );
    assert_eq!(inbox.quarantined, 1);
    let waiting = test
        .hub
        .store()
        .reports(KeyStatus::Quarantine, 0, false, 10)
        .unwrap();
    assert_eq!(
        (
            waiting.len(),
            waiting[0].id,
            waiting[0].report.title.as_str()
        ),
        (1, first, "push hid git's error")
    );
    assert_eq!(
        admin_ack(&test, &token, first).await,
        404,
        "a quarantined report cannot be acked over the API"
    );

    allow(&test, &key);
    let second = accepted(&test, &upload(&test, &key, report("board froze")).await).await;
    assert!(second > first);
    let inbox: ReportInbox = body(admin_get(&test, Some(&token), 0).await).await;
    assert_eq!(inbox.quarantined, 0);
    assert_eq!(
        inbox
            .reports
            .iter()
            .map(|received| (received.id, received.report.title.as_str()))
            .collect::<Vec<_>>(),
        [(first, "push hid git's error"), (second, "board froze")],
        "allowing the key lets its waiting report through too"
    );
    assert_eq!(inbox.reports[0].key, Bytes32(key.public()));
    assert_eq!(inbox.reports[1].report, report("board froze"));
    let after: ReportInbox = body(admin_get(&test, Some(&token), first).await).await;
    assert_eq!(
        after.reports.len(),
        1,
        "after skips what the caller has seen"
    );

    assert_eq!(admin_ack(&test, &token, first).await, 204);
    assert_eq!(
        admin_ack(&test, &token, first).await,
        204,
        "acking twice is harmless"
    );
    let inbox: ReportInbox = body(admin_get(&test, Some(&token), 0).await).await;
    assert_eq!(
        inbox
            .reports
            .iter()
            .map(|received| received.id)
            .collect::<Vec<_>>(),
        [second],
        "an acked report is not handed out again"
    );
    assert_eq!(admin_ack(&test, &token, 9_999).await, 404);
}

#[tokio::test]
async fn the_admin_api_takes_only_an_admin_token() {
    let test = hub().await;
    let token = test
        .hub
        .mint_admin_token("laptop", AdminScope::Reports)
        .unwrap()
        .unwrap();
    assert_eq!(
        refusal(admin_get(&test, None, 0).await).await,
        (401, "unauthorized".to_string())
    );
    assert_eq!(
        refusal(admin_get(&test, Some("kha_not-a-token"), 0).await)
            .await
            .0,
        401
    );
    let device = common::signed_up(&test).await.register_device().await;
    assert_eq!(
        refusal(admin_get(&test, Some(&device.token), 0).await)
            .await
            .0,
        401,
        "a device token is not an admin token"
    );
    let ack = reqwest::Client::new()
        .post(format!("{}/v1/admin/reports/1/ack", test.url))
        .send()
        .await
        .unwrap();
    assert_eq!(ack.status().as_u16(), 401);
    assert_eq!(
        admin_get(&test, Some(&token), 0).await.status().as_u16(),
        200
    );
    let stored = common::database_bytes(&test);
    assert!(
        !common::holds(&stored, token.as_bytes()),
        "the token is kept as a hash"
    );
}

#[tokio::test]
async fn a_bad_signature_a_forged_or_expired_or_spent_challenge_and_too_little_work_are_refused() {
    let test = hub().await;
    let key = install_key(1);

    let mut forged = upload(&test, &key, report("one")).await;
    forged.signature = Bytes64(install_key(2).sign(&forged.challenge.nonce.0, &forged.report));
    assert_eq!(
        refusal(post(&test, &forged).await).await,
        (400, "bad-signature".to_string())
    );
    let mut edited = upload(&test, &key, report("one")).await;
    edited.report.text.push_str(" edited after signing");
    edited.counter = solve(
        &edited.challenge.nonce.0,
        &edited.report.digest(),
        DIFFICULTY,
    );
    assert_eq!(
        refusal(post(&test, &edited).await).await,
        (400, "bad-signature".to_string())
    );

    let mut easier = challenge(&test).await;
    easier.difficulty = 0;
    assert_eq!(
        refusal(post(&test, &signed(&key, report("one"), easier)).await).await,
        (400, "bad-challenge".to_string()),
        "a lowered difficulty breaks the tag"
    );
    let mut longer = challenge(&test).await;
    longer.expires += 3_600;
    assert_eq!(
        refusal(post(&test, &signed(&key, report("one"), longer)).await).await,
        (400, "bad-challenge".to_string())
    );
    let mut own = challenge(&test).await;
    own.nonce = Bytes32([5; 32]);
    assert_eq!(
        refusal(post(&test, &signed(&key, report("one"), own)).await).await,
        (400, "bad-challenge".to_string())
    );

    let mut lazy = upload(&test, &key, report("one")).await;
    let digest = lazy.report.digest();
    lazy.counter = (0..)
        .find(|counter| !meets(&lazy.challenge.nonce.0, &digest, *counter, DIFFICULTY))
        .unwrap();
    assert_eq!(
        refusal(post(&test, &lazy).await).await,
        (400, "insufficient-work".to_string())
    );

    let once = upload(&test, &key, report("one")).await;
    accepted(&test, &once).await;
    assert_eq!(
        refusal(post(&test, &once).await).await,
        (409, "challenge-used".to_string())
    );
    let again = signed(&key, report("another"), once.challenge);
    assert_eq!(
        refusal(post(&test, &again).await).await,
        (409, "challenge-used".to_string()),
        "a challenge is spent whatever the report"
    );
    assert_eq!(
        rows(&test, "reports"),
        1,
        "only the one valid report was stored"
    );

    let expiring = start_hub(|config| {
        config.report_difficulty = DIFFICULTY;
        config.report_challenge_ttl = 0;
    })
    .await;
    let stale = upload(&expiring, &key, report("one")).await;
    assert_eq!(
        refusal(post(&expiring, &stale).await).await,
        (400, "expired-challenge".to_string())
    );
    assert_eq!(rows(&expiring, "reports"), 0);
}

#[tokio::test]
async fn an_oversized_report_is_refused() {
    let test = hub().await;
    let key = install_key(1);
    let mut long = report("long");
    long.text = "x".repeat(MAX_TEXT_BYTES + 1);
    assert_eq!(
        refusal(post(&test, &upload(&test, &key, long).await).await).await,
        (413, "too-large".to_string())
    );
    let mut huge = report("huge");
    huge.text = "\u{1}".repeat(4 * 1024);
    let response = post(&test, &upload(&test, &key, huge).await).await;
    assert_eq!(
        response.status().as_u16(),
        413,
        "a body over 16 KiB is cut off before it is read"
    );
    let mut fits = report("fits");
    fits.text = "x".repeat(MAX_TEXT_BYTES);
    accepted(&test, &upload(&test, &key, fits).await).await;
    let mut bad = report("bad");
    bad.title = "two\nlines".to_string();
    assert_eq!(
        refusal(post(&test, &upload(&test, &key, bad).await).await).await,
        (400, "bad-title".to_string())
    );
}

#[tokio::test]
async fn reports_are_rate_limited_per_install_key_and_per_address() {
    let test = start_hub(|config| {
        config.report_difficulty = DIFFICULTY;
        config.report_key_rate = RateLimit {
            burst: 2,
            per_minute: 1,
        };
        config.report_address_rate = RateLimit {
            burst: 4,
            per_minute: 1,
        };
    })
    .await;
    let key = install_key(1);
    for title in ["one", "two"] {
        accepted(&test, &upload(&test, &key, report(title)).await).await;
    }
    assert_eq!(
        refusal(post(&test, &upload(&test, &key, report("three")).await).await).await,
        (429, "rate-limited".to_string())
    );
    let other = install_key(2);
    accepted(
        &test,
        &upload(&test, &other, report("from another install")).await,
    )
    .await;
    assert_eq!(
        refusal(
            post(
                &test,
                &upload(&test, &install_key(3), report("fifth from this address")).await
            )
            .await
        )
        .await,
        (429, "rate-limited".to_string()),
        "the address has spent its own bucket"
    );
    assert_eq!(rows(&test, "reports"), 3);
}

#[tokio::test]
async fn behind_cloudflare_the_address_limit_follows_the_client_not_the_proxy() {
    let test = start_hub(|config| {
        config.report_difficulty = DIFFICULTY;
        config.trusted_proxy = Some(kebbie_hub::TrustedProxy::Cloudflare);
        config.report_address_rate = RateLimit {
            burst: 1,
            per_minute: 1,
        };
    })
    .await;
    let send = async |client: &str, upload: &ReportUpload| {
        reqwest::Client::new()
            .post(format!("{}/v1/reports", test.url))
            .header("cf-connecting-ip", client)
            .json(upload)
            .send()
            .await
            .unwrap()
            .status()
            .as_u16()
    };
    assert_eq!(
        send(
            "192.0.2.1",
            &upload(&test, &install_key(1), report("one")).await
        )
        .await,
        202
    );
    assert_eq!(
        send(
            "192.0.2.1",
            &upload(&test, &install_key(2), report("two")).await
        )
        .await,
        429
    );
    assert_eq!(
        send(
            "192.0.2.2",
            &upload(&test, &install_key(3), report("three")).await
        )
        .await,
        202
    );
}

#[tokio::test]
async fn a_blocked_keys_reports_are_accepted_as_any_other_and_dropped() {
    let test = hub().await;
    let token = test
        .hub
        .mint_admin_token("laptop", AdminScope::Reports)
        .unwrap()
        .unwrap();
    let blocked = install_key(1);
    let waiting = accepted(&test, &upload(&test, &blocked, report("spam one")).await).await;
    test.hub
        .store()
        .set_key_status(&blocked.public(), KeyStatus::Blocked, kebbie_hub::now())
        .unwrap();
    assert_eq!(rows(&test, "reports"), 0, "blocking drops what waited");
    let response = post(&test, &upload(&test, &blocked, report("spam two")).await).await;
    assert_eq!(response.status().as_u16(), 202);
    let dropped: Value = response.json().await.unwrap();
    let dropped_id = dropped["report"].as_u64().unwrap();
    assert!(
        dropped_id > waiting && dropped.as_object().unwrap().len() == 1,
        "the answer has the shape of an accepted one: {dropped}"
    );
    assert_eq!(rows(&test, "reports"), 0, "nothing is kept");
    let allowed = install_key(2);
    allow(&test, &allowed);
    let next = accepted(&test, &upload(&test, &allowed, report("real")).await).await;
    assert!(
        next > dropped_id,
        "a dropped report spends its id like a stored one, so ids do not tell"
    );
    let inbox: ReportInbox = body(admin_get(&test, Some(&token), 0).await).await;
    assert_eq!((inbox.reports.len(), inbox.quarantined), (1, 0));
    let events = test.hub.store().events().unwrap();
    assert!(
        events.iter().any(|(kind, detail)| kind == "report.received"
            && detail
                .as_deref()
                .is_some_and(|detail| detail.contains("dropped-blocked"))),
        "{events:?}"
    );
    assert!(
        !events.iter().any(|(_, detail)| detail
            .as_deref()
            .is_some_and(|detail| detail.contains("spam"))),
        "the event log holds no report text"
    );
}

#[tokio::test]
async fn a_revoked_token_stops_working_at_once() {
    let test = hub().await;
    let token = test
        .hub
        .mint_admin_token("laptop", AdminScope::Reports)
        .unwrap()
        .unwrap();
    assert_eq!(
        admin_get(&test, Some(&token), 0).await.status().as_u16(),
        200
    );
    assert!(
        test.hub
            .mint_admin_token("laptop", AdminScope::Reports)
            .unwrap()
            .is_none(),
        "names are unique"
    );
    let used = test.hub.store().admin_tokens().unwrap();
    assert!(used[0].name == "laptop" && used[0].last_used.is_some());
    assert!(test.hub.store().revoke_admin_token("laptop").unwrap());
    assert_eq!(
        admin_get(&test, Some(&token), 0).await.status().as_u16(),
        401
    );
}

#[tokio::test]
async fn one_unknown_key_fills_no_more_than_its_share_of_quarantine() {
    let test = start_hub(|config| {
        config.report_difficulty = DIFFICULTY;
        config.max_quarantined_per_key = 2;
        config.report_key_rate = RateLimit {
            burst: 10,
            per_minute: 10,
        };
    })
    .await;
    let noisy = install_key(1);
    for title in ["one", "two", "three"] {
        accepted(&test, &upload(&test, &noisy, report(title)).await).await;
    }
    assert_eq!(
        rows(&test, "reports"),
        2,
        "the third from one key is dropped, answered like the rest"
    );
    accepted(
        &test,
        &upload(&test, &install_key(2), report("another sender")).await,
    )
    .await;
    assert_eq!(rows(&test, "reports"), 3);
    allow(&test, &noisy);
    accepted(&test, &upload(&test, &noisy, report("allowed now")).await).await;
    assert_eq!(
        rows(&test, "reports"),
        4,
        "an allowed key has no quarantine cap"
    );
}

#[tokio::test]
async fn a_full_quarantine_drops_further_unknown_reports_but_not_allowed_ones() {
    let test = start_hub(|config| {
        config.report_difficulty = DIFFICULTY;
        config.max_quarantined = 1;
    })
    .await;
    accepted(
        &test,
        &upload(&test, &install_key(1), report("first")).await,
    )
    .await;
    accepted(
        &test,
        &upload(&test, &install_key(2), report("second")).await,
    )
    .await;
    assert_eq!(rows(&test, "reports"), 1);
    let known = install_key(3);
    allow(&test, &known);
    accepted(&test, &upload(&test, &known, report("allowed")).await).await;
    assert_eq!(rows(&test, "reports"), 2);
}

#[test]
fn a_schema_4_database_is_upgraded_in_place_and_an_older_one_refused() {
    let data = tempfile::tempdir().unwrap();
    let path = data.path().join("hub.sqlite3");
    let store = Store::open(&path).unwrap();
    store.add_join_code(b"kept", u64::MAX >> 2).unwrap();
    drop(store);
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("DROP TABLE reports; DROP TABLE report_keys; DROP TABLE report_challenges; DROP TABLE admin_tokens; PRAGMA user_version = 4;").unwrap();
    drop(connection);
    let store = Store::open(&path).unwrap();
    store
        .set_key_status(&[1; 32], KeyStatus::Allowed, 1)
        .unwrap();
    drop(store);
    let connection = rusqlite::Connection::open(&path).unwrap();
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    let kept: i64 = connection
        .query_row("SELECT count(*) FROM join_codes", [], |row| row.get(0))
        .unwrap();
    assert_eq!((version, kept), (9, 1), "the upgrade keeps what was there");
    connection
        .execute_batch("PRAGMA user_version = 3;")
        .unwrap();
    drop(connection);
    assert!(
        Store::open(&path)
            .err()
            .unwrap()
            .to_string()
            .contains("schema 3")
    );
}

#[test]
fn the_admin_cli_mints_a_token_lists_reports_and_decides_on_keys() {
    let data = tempfile::tempdir().unwrap();
    let public = install_key(1).public();
    let mut store = Store::open(&kebbie_hub::database_path(data.path())).unwrap();
    store
        .store_report(&kebbie_hub::store::NewReport {
            key: &public,
            report: &report("waiting"),
            now: 1,
            max_quarantined: 10,
            max_quarantined_per_key: 10,
        })
        .unwrap();
    drop(store);
    let run = |args: &[&str]| {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_kebbie-hub"))
            .arg("--data")
            .arg(data.path())
            .arg("admin")
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        (
            String::from_utf8_lossy(&output.stdout).to_string(),
            String::from_utf8_lossy(&output.stderr).to_string(),
        )
    };
    let (token, _) = run(&["token", "--name", "laptop"]);
    assert!(token.trim().starts_with("kha_"));
    let taken = std::process::Command::new(env!("CARGO_BIN_EXE_kebbie-hub"))
        .arg("--data")
        .arg(data.path())
        .args(["admin", "token", "--name", "laptop"])
        .output()
        .unwrap();
    assert!(
        !taken.status.success()
            && String::from_utf8_lossy(&taken.stderr).contains("a token named laptop exists")
    );
    let (listed, _) = run(&["token", "list"]);
    assert!(
        listed.starts_with("laptop  reports  made ")
            && listed.contains("never used")
            && !listed.contains("kha_"),
        "{listed}"
    );
    let (listed, told) = run(&["reports"]);
    assert!(
        listed.is_empty()
            && told.contains("1 more wait in quarantine: kebbie-hub admin reports --quarantine"),
        "{listed}{told}"
    );
    let (quarantined, _) = run(&["reports", "--quarantine", "--json"]);
    let rows: Value = serde_json::from_str(&quarantined).unwrap();
    let shown_key = URL_SAFE_NO_PAD.encode(public);
    assert_eq!(
        (
            rows[0]["title"].as_str(),
            rows[0]["status"].as_str(),
            rows[0]["key"].as_str()
        ),
        (
            Some("waiting"),
            Some("quarantine"),
            Some(shown_key.as_str())
        )
    );
    run(&["reports", "allow", &shown_key]);
    let (listed, _) = run(&["reports"]);
    assert!(
        listed.contains("waiting") && listed.contains("allowed"),
        "{listed}"
    );
    let id = rows[0]["id"].as_u64().unwrap().to_string();
    run(&["reports", "ack", &id]);
    let (listed, _) = run(&["reports"]);
    assert!(
        listed.is_empty(),
        "an acked report leaves the list: {listed}"
    );
    run(&["reports", "block", &shown_key]);
    let store = Store::open(&kebbie_hub::database_path(data.path())).unwrap();
    assert!(
        store
            .reports(KeyStatus::Blocked, 0, false, 10)
            .unwrap()
            .is_empty()
    );
}
