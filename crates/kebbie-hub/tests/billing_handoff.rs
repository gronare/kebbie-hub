#[allow(dead_code)]
mod common;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use common::*;
use hmac::{Hmac, KeyInit, Mac};
use kebbie_hub::billing::{BillingPage, HANDOFF_DOMAIN, HandoffSecret, handoff_link, signature};
use kebbie_hub_fakes::browser::Browser;
use serde_json::Value;
use sha2::Sha256;

const VECTOR: &str = include_str!("../../../vectors/billing-handoff.json");
const BILLING: &str = "https://billing.example.com/";

fn vector() -> Value {
    serde_json::from_str(VECTOR).unwrap()
}

fn vector_secret() -> HandoffSecret {
    HandoffSecret::parse(vector()["secret"].as_str().unwrap()).unwrap()
}

fn handing_off(config: &mut kebbie_hub::Config) {
    config.billing_url = Some(BILLING.to_string());
    config.billing_handoff_secret = Some(vector_secret());
}

struct Hop {
    status: u16,
    location: String,
    cache: String,
}

async fn visit(person: &Browser, path: &str) -> Hop {
    let mut request = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
        .get(format!("{}{path}", person.hub));
    if let Some(cookie) = &person.cookie {
        request = request.header("cookie", cookie);
    }
    let response = request.send().await.unwrap();
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .map(|value| value.to_str().unwrap().to_string())
            .unwrap_or_default()
    };
    Hop {
        status: response.status().as_u16(),
        location: header("location"),
        cache: header("cache-control"),
    }
}

#[test]
fn the_signature_matches_the_billing_apps_vector() {
    let vector = vector();
    assert_eq!(vector["domain"], HANDOFF_DOMAIN);
    let secret = vector_secret();
    let raw = URL_SAFE_NO_PAD.decode(vector["secret"].as_str().unwrap()).unwrap();
    let account = vector["account"].as_str().unwrap();
    let expires = vector["expires"].as_u64().unwrap();
    for case in vector["cases"].as_array().unwrap() {
        let page = BillingPage::parse(case["purpose"].as_str().unwrap()).unwrap();
        let expected = case["signature"].as_str().unwrap();
        assert_eq!(signature(&secret, page, account, expires), expected);
        let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(&raw).unwrap();
        mac.update(
            format!("{HANDOFF_DOMAIN}{}\0{account}\0{expires}", page.purpose()).as_bytes(),
        );
        assert_eq!(URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()), expected);
        assert_eq!(
            handoff_link("https://billing.example.com/", &secret, page, account, expires),
            format!(
                "https://billing.example.com/{}?account={account}&expires={expires}&signature={expected}",
                case["path"].as_str().unwrap()
            )
        );
    }
}

#[test]
fn a_handoff_secret_is_32_bytes_of_base64url_and_never_printed() {
    let text = vector()["secret"].as_str().unwrap().to_string();
    assert!(HandoffSecret::parse(&format!(" {text}=\n")).is_ok());
    assert!(HandoffSecret::parse(&URL_SAFE_NO_PAD.encode([7u8; 31])).is_err());
    assert!(HandoffSecret::parse("not base64!").is_err());
    let shown = format!("{:?}", vector_secret());
    assert!(!shown.contains(&text) && !shown.contains("104"), "{shown}");
}

#[tokio::test]
async fn a_signed_in_account_is_sent_to_its_own_signed_checkout_and_portal() {
    let test = start_hub(handing_off).await;
    let mut person = member(&test).await;
    let account = person.account().await.unwrap().account;
    let secret = vector_secret();
    for (purpose, page, path) in [
        ("checkout", BillingPage::Checkout, "checkout"),
        ("portal", BillingPage::Portal, "billing_portal"),
    ] {
        let before = kebbie_hub::now();
        let hop = visit(&person, &format!("/v1/a/billing/{purpose}")).await;
        assert_eq!(hop.status, 303, "{purpose}");
        assert_eq!(hop.cache, "no-store");
        let prefix = format!("https://billing.example.com/{path}?account={account}&expires=");
        let rest = hop.location.strip_prefix(&prefix).unwrap_or_else(|| panic!("{}", hop.location));
        let (expires, sent) = rest.split_once("&signature=").unwrap();
        let expires: u64 = expires.parse().unwrap();
        assert!(expires > before && expires <= kebbie_hub::now() + 600, "{expires}");
        assert_eq!(sent, signature(&secret, page, &account, expires));
    }
    assert_eq!(visit(&person, "/v1/a/billing/refund").await.status, 404);
}

#[tokio::test]
async fn a_visit_without_a_session_goes_through_sign_in_and_back() {
    let test = start_hub(handing_off).await;
    let mut stale = browser(&test);
    stale.cookie = Some("kebbie_session=khs_expired".to_string());
    for person in [browser(&test), stale] {
        for purpose in ["checkout", "portal"] {
            let hop = visit(&person, &format!("/v1/a/billing/{purpose}")).await;
            assert_eq!(hop.status, 303, "{purpose}");
            assert_eq!(hop.location, format!("/?next=%2Fv1%2Fa%2Fbilling%2F{purpose}"));
            assert_eq!(hop.cache, "no-store");
        }
        assert_eq!(visit(&person, "/v1/a/billing/refund").await.status, 404);
    }
    let home = reqwest::get(format!("{}/?next=%2Fv1%2Fa%2Fbilling%2Fcheckout", test.url)).await.unwrap();
    assert_eq!(home.status(), 200, "the sign-in page takes the query");
}

#[tokio::test]
async fn without_a_billing_url_and_a_secret_there_is_no_handoff() {
    for configure in [
        (|config: &mut kebbie_hub::Config| config.billing_url = Some(BILLING.to_string()))
            as fn(&mut kebbie_hub::Config),
        |config| config.billing_handoff_secret = Some(vector_secret()),
        |_| {},
    ] {
        let test = start_hub(configure).await;
        let person = member(&test).await;
        let hop = visit(&person, "/v1/a/billing/checkout").await;
        assert_eq!((hop.status, hop.location), (404, String::new()));
    }
}

#[tokio::test]
async fn an_inactive_plan_points_at_the_hubs_handoff_when_it_signs_links() {
    let test = start_hub(|config| {
        handing_off(config);
        config.plans = true;
    })
    .await;
    let mut person = member(&test).await;
    let entry = "http://localhost:8470/v1/a/billing/checkout";
    assert_eq!(person.account().await.unwrap().billing_url.as_deref(), Some(entry));
    let pending = kebbie_hub_fakes::machine::connect(&test.url, "mac").await.unwrap();
    match person.approve_connect(&pending.link(), false).await {
        Err(kebbie_hub_fakes::HubError::PlanInactive { url }) => assert_eq!(url.as_deref(), Some(entry)),
        other => panic!("an inactive account connected: {:?}", other.map(|_| ())),
    }
}

#[test]
fn the_hub_refuses_to_start_with_a_malformed_handoff_secret() {
    let data = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_kebbie-hub"))
        .arg("--dev")
        .arg("--data")
        .arg(data.path())
        .arg("--listen")
        .arg("127.0.0.1:0")
        .arg("--billing-url")
        .arg(BILLING)
        .env("KEBBIE_HUB_BILLING_HANDOFF_SECRET", "c2hvcnQ")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("KEBBIE_HUB_BILLING_HANDOFF_SECRET") && !said.contains("c2hvcnQ"), "{said}");
}
