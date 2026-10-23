#[allow(dead_code)]
mod common;

use kelpie_hub::store::{KeyStatus, SignalRow, Store};
use kelpie_hub_api::{
    Bytes32, Bytes64, ReportChallenge, ReportUpload, SignalUpload, SignalsAccepted,
};
use kelpie_hub_protocol::{
    BugReport, ErrorSignals, InstallKey, Name, SignalCount, SignalKind, solve,
};
use reqwest::Response;
use serde_json::{Value, json};

use common::{TestHub, body, refusal, rows, start_hub};

const DIFFICULTY: u8 = 8;

async fn hub() -> TestHub {
    start_hub(|config| config.report_difficulty = DIFFICULTY).await
}

fn name(text: &str) -> Name {
    Name::new(text).unwrap()
}

fn signals(rows: &[(&str, SignalKind, u32)]) -> ErrorSignals {
    ErrorSignals {
        version: name("0.26.3"),
        commit: Some(name("16983b9")),
        os: name("macos"),
        arch: name("aarch64"),
        counts: rows
            .iter()
            .map(|(component, kind, count)| SignalCount {
                component: name(component),
                kind: *kind,
                count: *count,
            })
            .collect(),
    }
}

fn key(seed: u8) -> InstallKey {
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

async fn upload(test: &TestHub, key: &InstallKey, signals: ErrorSignals) -> SignalUpload {
    let challenge = challenge(test).await;
    SignalUpload {
        counter: solve(&challenge.nonce.0, &signals.digest(), challenge.difficulty),
        signature: Bytes64(key.sign_signals(&challenge.nonce.0, &signals)),
        key: Bytes32(key.public()),
        signals,
        challenge,
    }
}

async fn post(test: &TestHub, body: &Value) -> Response {
    reqwest::Client::new()
        .post(format!("{}/v1/signals", test.url))
        .json(body)
        .send()
        .await
        .unwrap()
}

async fn counted(test: &TestHub, upload: &SignalUpload) -> usize {
    let response = post(test, &serde_json::to_value(upload).unwrap()).await;
    assert_eq!(response.status().as_u16(), 202);
    body::<SignalsAccepted>(response).await.rows
}

fn listed(test: &TestHub) -> Vec<(String, String, bool, u64)> {
    test.hub
        .store()
        .signal_rows(0)
        .unwrap()
        .into_iter()
        .map(|row: SignalRow| (row.component, row.kind, row.vetted, row.count))
        .collect()
}

#[tokio::test]
async fn signals_from_an_unknown_install_count_apart_until_its_key_is_allowed_and_a_blocked_one_counts_nowhere()
 {
    let test = hub().await;
    let stranger = key(1);
    let sent = signals(&[
        ("keeper.push", SignalKind::InternalError, 3),
        ("board.draw", SignalKind::Panic, 1),
    ]);
    assert_eq!(
        counted(&test, &upload(&test, &stranger, sent.clone()).await).await,
        2
    );
    assert_eq!(
        listed(&test),
        [
            (
                "keeper.push".to_string(),
                "internal-error".to_string(),
                false,
                3
            ),
            ("board.draw".to_string(), "panic".to_string(), false, 1),
        ]
    );
    assert_eq!(
        rows(&test, "report_keys"),
        0,
        "a signal leaves no standing for its key"
    );

    test.hub
        .store()
        .set_key_status(&stranger.public(), KeyStatus::Allowed, 1)
        .unwrap();
    counted(&test, &upload(&test, &stranger, sent.clone()).await).await;
    counted(
        &test,
        &upload(
            &test,
            &stranger,
            signals(&[("keeper.push", SignalKind::InternalError, 2)]),
        )
        .await,
    )
    .await;
    let vetted: Vec<_> = listed(&test).into_iter().filter(|row| row.2).collect();
    assert_eq!(
        vetted,
        [
            (
                "keeper.push".to_string(),
                "internal-error".to_string(),
                true,
                5
            ),
            ("board.draw".to_string(), "panic".to_string(), true, 1),
        ],
        "an allowed key's counts add up per version, OS, component and kind"
    );

    let blocked = key(2);
    test.hub
        .store()
        .set_key_status(&blocked.public(), KeyStatus::Blocked, 1)
        .unwrap();
    let before = listed(&test);
    assert_eq!(
        counted(&test, &upload(&test, &blocked, sent).await).await,
        2,
        "a blocked key is answered like any other"
    );
    assert_eq!(listed(&test), before);
    assert_eq!(
        rows(&test, "signal_senders"),
        1,
        "nothing of a blocked sender is kept"
    );
}

#[tokio::test]
async fn the_hub_takes_no_field_beyond_the_schema_and_no_text_in_any() {
    let test = hub().await;
    let sender = key(1);
    let good = serde_json::to_value(
        upload(
            &test,
            &sender,
            signals(&[("keeper.push", SignalKind::InternalError, 1)]),
        )
        .await,
    )
    .unwrap();
    let changed = |change: &dyn Fn(&mut Value)| {
        let mut value = good.clone();
        change(&mut value);
        value
    };
    for refused in [
        changed(&|value| value["message"] = json!("push failed")),
        changed(&|value| value["signals"]["message"] = json!("push failed in ~/work/app")),
        changed(&|value| value["signals"]["counts"][0]["detail"] = json!("git said no")),
        changed(&|value| value["signals"]["counts"][0]["component"] = json!("/Users/carl/app")),
        changed(&|value| value["signals"]["counts"][0]["component"] = json!("keeper push")),
        changed(&|value| value["signals"]["counts"][0]["component"] = json!("Keeper.Push")),
        changed(&|value| value["signals"]["counts"][0]["component"] = json!("x".repeat(49))),
        changed(&|value| value["signals"]["counts"][0]["kind"] = json!("network-down")),
        changed(&|value| value["signals"]["counts"][0]["count"] = json!(-1)),
        changed(&|value| value["signals"]["os"] = json!("macOS 15 on carl's laptop")),
        changed(&|value| value["signals"]["version"] = json!("")),
    ] {
        let response = post(&test, &refused).await;
        assert_eq!(
            response.status().as_u16(),
            422,
            "{refused}: {}",
            response.status()
        );
    }
    assert_eq!(rows(&test, "signal_counts"), 0);
    assert_eq!(rows(&test, "signal_senders"), 0);
    let response = post(&test, &good).await;
    assert_eq!(response.status().as_u16(), 202, "the untouched one goes in");
}

#[tokio::test]
async fn a_bad_shape_signature_or_work_is_refused_and_a_challenge_is_spent_once() {
    let test = hub().await;
    let sender = key(1);
    for (sent, code) in [
        (signals(&[]), "no-signals"),
        (signals(&[("a", SignalKind::Panic, 0)]), "bad-count"),
        (
            signals(&[("a", SignalKind::Panic, 1), ("a", SignalKind::Panic, 2)]),
            "repeated-signal",
        ),
    ] {
        let upload = upload(&test, &sender, sent).await;
        assert_eq!(
            refusal(post(&test, &serde_json::to_value(&upload).unwrap()).await).await,
            (400, code.to_string())
        );
    }
    let mut forged = upload(&test, &sender, signals(&[("a", SignalKind::Panic, 1)])).await;
    forged.signals.counts[0].count = 9;
    forged.counter = solve(
        &forged.challenge.nonce.0,
        &forged.signals.digest(),
        DIFFICULTY,
    );
    assert_eq!(
        refusal(post(&test, &serde_json::to_value(&forged).unwrap()).await).await,
        (400, "bad-signature".to_string())
    );
    let mut lazy = upload(&test, &sender, signals(&[("a", SignalKind::Panic, 1)])).await;
    lazy.counter = (0..)
        .find(|counter| {
            !kelpie_hub_protocol::meets(
                &lazy.challenge.nonce.0,
                &lazy.signals.digest(),
                *counter,
                DIFFICULTY,
            )
        })
        .unwrap();
    assert_eq!(
        refusal(post(&test, &serde_json::to_value(&lazy).unwrap()).await).await,
        (400, "insufficient-work".to_string())
    );

    let once = upload(&test, &sender, signals(&[("a", SignalKind::Panic, 1)])).await;
    counted(&test, &once).await;
    assert_eq!(
        refusal(post(&test, &serde_json::to_value(&once).unwrap()).await).await,
        (409, "challenge-used".to_string())
    );
    let report = BugReport {
        title: "t".to_string(),
        text: "x".to_string(),
        version: "0.26.3".to_string(),
        os: "macos aarch64".to_string(),
    };
    let as_report = ReportUpload {
        counter: solve(&once.challenge.nonce.0, &report.digest(), DIFFICULTY),
        signature: Bytes64(sender.sign(&once.challenge.nonce.0, &report)),
        key: once.key,
        report,
        challenge: once.challenge,
    };
    let response = reqwest::Client::new()
        .post(format!("{}/v1/reports", test.url))
        .json(&as_report)
        .send()
        .await
        .unwrap();
    assert_eq!(
        refusal(response).await,
        (409, "challenge-used".to_string()),
        "a challenge spent on signals buys no report"
    );
    let signed_as_report = Bytes64(sender.sign(
        &[0; 32],
        &BugReport {
            title: "t".to_string(),
            text: String::new(),
            version: "v".to_string(),
            os: "o".to_string(),
        },
    ));
    let mut crossed = upload(&test, &sender, signals(&[("a", SignalKind::Panic, 1)])).await;
    crossed.signature = signed_as_report;
    assert_eq!(
        refusal(post(&test, &serde_json::to_value(&crossed).unwrap()).await).await,
        (400, "bad-signature".to_string())
    );
}

#[tokio::test]
async fn one_install_sends_a_few_batches_a_day_and_unknown_ones_fill_no_more_than_their_cap() {
    let test = start_hub(|config| {
        config.report_difficulty = DIFFICULTY;
        config.max_signal_batches_per_key = 2;
        config.max_unvetted_signal_rows = 2;
    })
    .await;
    let sender = key(1);
    let one = signals(&[("a", SignalKind::Panic, 1)]);
    for _ in 0..3 {
        counted(&test, &upload(&test, &sender, one.clone()).await).await;
    }
    assert_eq!(
        listed(&test)[0].3,
        2,
        "the third batch of the day is dropped"
    );
    let wide = signals(&[("b", SignalKind::Panic, 1), ("c", SignalKind::Panic, 1)]);
    counted(&test, &upload(&test, &key(2), wide.clone()).await).await;
    assert_eq!(
        rows(&test, "signal_counts"),
        1,
        "past the unvetted cap nothing is added"
    );
    test.hub
        .store()
        .set_key_status(&key(3).public(), KeyStatus::Allowed, 1)
        .unwrap();
    counted(&test, &upload(&test, &key(3), wide).await).await;
    assert_eq!(
        rows(&test, "signal_counts"),
        3,
        "an allowed key is not held by the cap"
    );
}

#[tokio::test]
async fn a_flood_from_one_address_does_not_silence_another() {
    let test = start_hub(|config| {
        config.report_difficulty = DIFFICULTY;
        config.trusted_proxy = Some(kelpie_hub::TrustedProxy::Cloudflare);
        config.max_signal_batches_per_address = 3;
        config.report_address_rate = kelpie_hub::RateLimit {
            burst: 100,
            per_minute: 100,
        };
    })
    .await;
    let send = async |client: &str, upload: &SignalUpload| {
        let response = reqwest::Client::new()
            .post(format!("{}/v1/signals", test.url))
            .header("cf-connecting-ip", client)
            .json(upload)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 202);
    };
    for seed in 1..=10 {
        let flood = signals(&[(&format!("flood{seed}"), SignalKind::Panic, 1)]);
        send("192.0.2.1", &upload(&test, &key(seed), flood).await).await;
    }
    assert_eq!(
        rows(&test, "signal_counts"),
        3,
        "one address counts only its share of the day, whatever keys it mints"
    );
    send(
        "192.0.2.2",
        &upload(
            &test,
            &key(50),
            signals(&[("keeper.push", SignalKind::InternalError, 1)]),
        )
        .await,
    )
    .await;
    assert!(
        listed(&test).iter().any(|row| row.0 == "keeper.push"),
        "a real install elsewhere still counts"
    );
}

#[tokio::test]
async fn a_full_address_table_keeps_the_flooders_and_still_lets_a_newcomer_in() {
    let test = start_hub(|config| {
        config.max_buckets = 3;
        config.max_signal_batches_per_address = 2;
    })
    .await;
    let day = 20_000;
    let hub = &test.hub;
    assert!(
        hub.signal_batch_allowed("192.0.2.1", day) && hub.signal_batch_allowed("192.0.2.1", day)
    );
    assert!(!hub.signal_batch_allowed("192.0.2.1", day), "past its cap");
    assert!(
        hub.signal_batch_allowed("192.0.2.2", day) && hub.signal_batch_allowed("192.0.2.3", day)
    );
    assert!(
        hub.signal_batch_allowed("192.0.2.4", day),
        "a full table still lets a new address in"
    );
    assert!(
        !hub.signal_batch_allowed("192.0.2.1", day),
        "an address at its cap stays counted"
    );
    assert!(
        hub.signal_batch_allowed("192.0.2.1", day + 1),
        "a new day starts over"
    );
}

#[test]
fn the_sweep_forgets_senders_when_the_day_turns_and_counts_after_half_a_year() {
    let data = tempfile::tempdir().unwrap();
    let mut store = Store::open(&data.path().join("hub.sqlite3")).unwrap();
    let day = 20_000;
    store
        .store_signals(&kelpie_hub::store::NewSignals {
            key: &[1; 32],
            signals: &signals(&[("a", SignalKind::Panic, 1)]),
            day,
            max_batches_per_key: 4,
            max_unvetted_rows: 10,
        })
        .unwrap();
    store.sweep(day * 86_400 + 86_399).unwrap();
    let senders = || -> i64 {
        rusqlite::Connection::open(data.path().join("hub.sqlite3"))
            .unwrap()
            .query_row("SELECT count(*) FROM signal_senders", [], |row| row.get(0))
            .unwrap()
    };
    assert_eq!(senders(), 1, "the day's senders are kept through the day");
    store.sweep((day + 1) * 86_400).unwrap();
    assert_eq!(senders(), 0, "who sent is forgotten when the day turns");
    assert_eq!(store.signal_rows(0).unwrap().len(), 1, "the counts stay");
    store
        .sweep((day + kelpie_hub::store::SIGNAL_KEEP_DAYS) * 86_400)
        .unwrap();
    assert!(store.signal_rows(0).unwrap().is_empty());
}

#[test]
fn a_schema_5_database_gains_the_signal_tables_and_keeps_its_reports() {
    let data = tempfile::tempdir().unwrap();
    let path = data.path().join("hub.sqlite3");
    let mut store = Store::open(&path).unwrap();
    let report = BugReport {
        title: "push hid git's error".to_string(),
        text: "kept across the upgrade".to_string(),
        version: "0.26.0".to_string(),
        os: "macos aarch64".to_string(),
    };
    store
        .store_report(&kelpie_hub::store::NewReport {
            key: &[1; 32],
            report: &report,
            now: 1,
            max_quarantined: 10,
            max_quarantined_per_key: 10,
        })
        .unwrap();
    store
        .set_key_status(&[1; 32], KeyStatus::Allowed, 1)
        .unwrap();
    drop(store);
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "DROP TABLE signal_counts; DROP TABLE signal_senders; PRAGMA user_version = 5;",
        )
        .unwrap();
    drop(connection);
    let mut store = Store::open(&path).unwrap();
    let kept = store.reports(KeyStatus::Allowed, 0, true, 10).unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].report, report);
    store
        .store_signals(&kelpie_hub::store::NewSignals {
            key: &[1; 32],
            signals: &signals(&[("a", SignalKind::Panic, 1)]),
            day: 1,
            max_batches_per_key: 4,
            max_unvetted_rows: 10,
        })
        .unwrap();
    assert!(
        store.signal_rows(0).unwrap()[0].vetted,
        "the key's standing carried over"
    );
    drop(store);
    let version: i64 = rusqlite::Connection::open(&path)
        .unwrap()
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 6);
}

#[test]
fn the_admin_cli_lists_the_counts_by_day() {
    let data = tempfile::tempdir().unwrap();
    let mut store = Store::open(&kelpie_hub::database_path(data.path())).unwrap();
    store
        .store_signals(&kelpie_hub::store::NewSignals {
            key: &[1; 32],
            signals: &signals(&[("keeper.push", SignalKind::InternalError, 3)]),
            day: kelpie_hub::now() / 86_400,
            max_batches_per_key: 4,
            max_unvetted_rows: 10,
        })
        .unwrap();
    drop(store);
    let run = |args: &[&str]| {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_kelpie-hub"))
            .arg("--data")
            .arg(data.path())
            .arg("admin")
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).to_string()
    };
    let listed = run(&["signals"]);
    assert!(
        listed.contains(
            "      3  keeper.push internal-error  kelpie 0.26.3 (16983b9)  macos aarch64  unvetted"
        ),
        "{listed}"
    );
    let json: Value = serde_json::from_str(&run(&["signals", "--json"])).unwrap();
    assert_eq!(
        (
            json[0]["component"].as_str(),
            json[0]["count"].as_u64(),
            json[0]["vetted"].as_bool()
        ),
        (Some("keeper.push"), Some(3), Some(false))
    );
}
