#[allow(dead_code)]
mod common;

use std::path::Path;

use common::*;
use kebbie_hub::web::ASSETS;
use kebbie_hub_api::{Boards, Bytes32, CommitRequest, Committed, PairingOpened, PairingView};
use kebbie_hub_fakes::{HubClient, HubError, PairError};
use tokio::sync::oneshot;

fn repository() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn shipped_files() -> Vec<String> {
    let mut files = Vec::new();
    for dir in ["web", "web/fonts", "js"] {
        for entry in std::fs::read_dir(repository().join(dir)).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().into_string().unwrap();
            if !entry.file_type().unwrap().is_dir() && !name.ends_with(".test.js") && !name.starts_with('.') {
                files.push(format!("{dir}/{name}"));
            }
        }
    }
    files.sort();
    files
}

#[tokio::test]
async fn the_hub_serves_the_web_app_from_its_own_origin() {
    let test = start_hub(|_| {}).await;
    let client = reqwest::Client::new();
    let page = client.get(format!("{}/", test.url)).send().await.unwrap();
    assert_eq!(page.status(), 200);
    assert_eq!(page.headers()["content-type"], "text/html; charset=utf-8");
    let policy = page.headers()["content-security-policy"]
        .to_str()
        .unwrap()
        .to_string();
    assert!(policy.contains("script-src 'self'"), "{policy}");
    assert!(policy.contains("connect-src 'self'"), "{policy}");
    assert!(policy.contains("frame-ancestors 'none'"), "{policy}");
    assert!(policy.contains("font-src 'self'") && policy.contains("style-src 'self';"), "the hub's own fonts and no inline styles: {policy}");
    assert_eq!(page.headers()["x-content-type-options"], "nosniff");
    let html = page.text().await.unwrap();
    assert!(html.contains(r#"<script type="module" src="/app.js">"#));
    assert!(html.contains(r#"<link rel="manifest" href="/manifest.webmanifest">"#));

    for (path, kind) in [
        ("/app.js", "text/javascript; charset=utf-8"),
        ("/js/protocol.js", "text/javascript; charset=utf-8"),
        ("/sw.js", "text/javascript; charset=utf-8"),
        ("/manifest.webmanifest", "application/manifest+json"),
        ("/apple-touch-icon.png", "image/png"),
    ] {
        let response = client
            .get(format!("{}{path}", test.url))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200, "{path}");
        assert_eq!(response.headers()["content-type"], kind, "{path}");
    }
    let manifest: serde_json::Value = client
        .get(format!("{}/manifest.webmanifest", test.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(manifest["display"], "standalone");
}

#[tokio::test]
async fn every_shipped_file_is_served_byte_for_byte_and_nothing_else() {
    let test = start_hub(|_| {}).await;
    let client = reqwest::Client::new();
    let mut served: Vec<String> = ASSETS
        .iter()
        .map(|(path, _, _)| match *path {
            "/" | "/setup" | "/signup" | "/connect" | "/add" | "/admin" | "/account" => {
                "web/index.html".to_string()
            }
            path if path.starts_with("/js/") => path.trim_start_matches('/').to_string(),
            path => format!("web{path}"),
        })
        .collect();
    served.sort();
    served.dedup();
    assert_eq!(served, shipped_files());
    for (path, _, bytes) in ASSETS {
        let body = client
            .get(format!("{}{path}", test.url))
            .send()
            .await
            .unwrap();
        assert_eq!(body.bytes().await.unwrap().as_ref(), *bytes, "{path}");
    }
    for path in [
        "/web.test.js",
        "/../Cargo.toml",
        "/node_modules/@hpke/core/esm/mod.js",
        "/v1/d/unknown",
    ] {
        let response = client
            .get(format!("{}{path}", test.url))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 404, "{path}");
    }
    let posted = client.post(format!("{}/", test.url)).send().await.unwrap();
    assert_eq!(posted.status(), 404);
}

#[tokio::test]
async fn a_board_entry_names_its_machine_its_keys_and_when_it_was_last_heard() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let mut person = member(&test).await;
    let mut machine = connected_with_board(&test, &mut person).await;
    let quiet = connected(&test, &mut person).await;
    machine.post_board(board(MARKER)).await.unwrap();
    let listing: Boards = person.get("/v1/a/boards").await.unwrap();
    assert_eq!(listing.boards.len(), 2);
    let entry = &listing.boards[0];
    assert_eq!((entry.name.as_str(), entry.machine_keys), ("mac", machine.public()));
    assert!(entry.rev.is_some());
    let list = listing.list.as_ref().unwrap();
    assert!(list.list.trusts(&machine.machine, &machine.public()));
    assert!(!list.list.trusts(&quiet.machine, &quiet.public()), "a machine connected without the handoff is not listed");
    let heard = entry.last_seen.unwrap();
    assert!(heard.abs_diff(kebbie_hub::now()) <= 5, "{heard}");
    let envelope = entry.envelope.as_ref().unwrap();
    let account = person.account_key().unwrap().account.clone();
    assert_eq!(envelope.keys.keys().collect::<Vec<_>>(), [&account], "only the account's row");
    let unread = &listing.boards[1];
    assert_eq!(unread.machine, quiet.machine);
    assert!(unread.rev.is_none() && unread.envelope.is_none());
    let mut stranger = member(&test).await;
    let theirs: Boards = stranger.get("/v1/a/boards").await.unwrap();
    assert!(theirs.boards.is_empty());
    assert_eq!(status_of(browser(&test).get::<Boards>("/v1/a/boards").await), 401);
}

#[tokio::test]
async fn a_phone_that_does_not_confirm_the_digits_trusts_no_machine() {
    let test = start_hub(|_| {}).await;
    let mut machine = joined_machine(&test).await;
    let mut phone = phone(&test).await;
    let (code_out, code_in) = oneshot::channel::<String>();
    let (admitted, paired) = tokio::join!(
        machine.pair(
            PAIRING_TIMEOUT,
            move |code| {
                let _ = code_out.send(code.to_string());
            },
            async |_| true
        ),
        async {
            let code = code_in.await.unwrap();
            phone.pair(&code, PAIRING_TIMEOUT, async |_| false).await
        }
    );
    admitted.unwrap();
    assert!(matches!(paired, Err(PairError::Declined)), "{paired:?}");
    assert!(phone.machines.is_empty());
    assert_eq!(phone.attempts.len(), 1);
}

#[tokio::test]
async fn a_removed_device_is_told_it_is_unknown_and_a_missing_token_is_not() {
    let test = start_hub(|_| {}).await;
    let person = signed_up(&test).await;
    let registered = person.register_device().await;
    let client = device_client(&test, &registered);
    let _: serde_json::Value = client.get("/v1/d/results", &[]).await.unwrap();
    let removed = person
        .delete(&format!("/v1/a/devices/{}", registered.device))
        .await;
    assert!(removed.status().is_success(), "{}", removed.status());
    assert_eq!(
        client.get::<serde_json::Value>("/v1/d/results", &[]).await.unwrap_err(),
        HubError::Status {
            status: 401,
            code: "unknown-device".to_string()
        }
    );
    assert_eq!(
        HubClient::new(&test.url, None)
            .get::<serde_json::Value>("/v1/d/results", &[])
            .await
            .unwrap_err(),
        HubError::Status {
            status: 401,
            code: "unauthorized".to_string()
        }
    );
}

#[tokio::test]
async fn the_phone_sees_the_machine_name_while_pairing() {
    let test = start_hub(|_| {}).await;
    let machine = joined_machine(&test).await;
    let phone = phone(&test).await;
    let opened: PairingOpened = machine.client().post("/v1/m/pairings", &()).await.unwrap();
    let committed: Committed = phone
        .client()
        .post(
            "/v1/d/pairings",
            &CommitRequest {
                code: opened.code,
                commitment: Bytes32([7; 32]),
            },
        )
        .await
        .unwrap();
    let view: PairingView = phone
        .client()
        .get(&format!("/v1/d/pairings/{}", committed.pairing), &[])
        .await
        .unwrap();
    assert_eq!(view.machine_name.as_deref(), Some("mac"));
}

#[tokio::test]
async fn the_health_check_answers_without_credentials_and_says_nothing() {
    let test = start_hub(|_| {}).await;
    let response = reqwest::get(format!("{}/healthz", test.url)).await.unwrap();
    assert_eq!(response.status(), 204);
    assert!(response.bytes().await.unwrap().is_empty());
}
