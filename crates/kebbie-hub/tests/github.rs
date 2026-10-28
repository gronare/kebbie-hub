#[allow(dead_code)]
mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use common::*;
use kebbie_hub::{Config, RateLimit, Secret};
use kebbie_hub_api::{AccountKeySetup, AccountView, Bytes64, HubInfo, SignupMode};
use kebbie_hub_fakes::browser::Browser;
use kebbie_hub_protocol::{AccountKey, Id, SecretKeys, Unlock, sign_wrapper};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use webauthn_rs::prelude::Url;

const CLIENT_ID: &str = "Iv1.kebbie-test";
const SECRET: &str = "github-client-secret-5c1f9e";

struct Grant {
    user: (u64, String),
    challenge: String,
    redirect_uri: String,
}

#[derive(Default)]
struct Fake {
    grants: HashMap<String, Grant>,
    tokens: HashMap<String, (u64, String)>,
    exchanges: usize,
    issued: u64,
}

#[derive(Clone)]
struct FakeGitHub {
    url: String,
    state: Arc<Mutex<Fake>>,
}

async fn token(State(fake): State<Arc<Mutex<Fake>>>, body: Bytes) -> Json<Value> {
    let request: Value = serde_json::from_slice(&body).unwrap();
    let mut fake = fake.lock().unwrap();
    fake.exchanges += 1;
    if request["client_id"] != CLIENT_ID || request["client_secret"] != SECRET {
        return Json(json!({"error": "incorrect_client_credentials"}));
    }
    let Some(grant) = fake.grants.remove(request["code"].as_str().unwrap_or_default()) else {
        return Json(json!({"error": "bad_verification_code"}));
    };
    let verifier = request["code_verifier"].as_str().unwrap_or_default();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    if challenge != grant.challenge || request["redirect_uri"] != grant.redirect_uri.as_str() {
        return Json(json!({"error": "bad_verification_code"}));
    }
    fake.issued += 1;
    let token = format!("gho_access{}", fake.issued);
    fake.tokens.insert(token.clone(), grant.user);
    Json(json!({"access_token": token, "token_type": "bearer", "scope": "read:user"}))
}

async fn user(
    State(fake): State<Arc<Mutex<Fake>>>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    let token = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let fake = fake.lock().unwrap();
    let (id, login) = fake.tokens.get(token).ok_or(StatusCode::UNAUTHORIZED)?;
    Ok(Json(json!({"id": id, "login": login, "email": "private@example.com"})))
}

async fn fake_github() -> FakeGitHub {
    let state = Arc::new(Mutex::new(Fake::default()));
    let router = axum::Router::new()
        .route("/login/oauth/access_token", post(token))
        .route("/user", get(user))
        .with_state(Arc::clone(&state));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    FakeGitHub { url, state }
}

impl FakeGitHub {
    fn approve(&self, authorize: &str, id: u64, login: &str) -> (String, String) {
        let url = Url::parse(authorize).unwrap();
        assert!(authorize.starts_with(&format!("{}/login/oauth/authorize?", self.url)), "{authorize}");
        let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(query["client_id"], CLIENT_ID);
        assert_eq!(query["scope"], "read:user");
        assert_eq!(query["code_challenge_method"], "S256");
        assert_eq!(query["redirect_uri"], "http://localhost:8470/auth/github/callback");
        let mut fake = self.state.lock().unwrap();
        let code = format!("code{}", fake.grants.len() + fake.tokens.len());
        fake.grants.insert(
            code.clone(),
            Grant {
                user: (id, login.to_string()),
                challenge: query["code_challenge"].clone(),
                redirect_uri: query["redirect_uri"].clone(),
            },
        );
        (code, query["state"].clone())
    }

    fn exchanges(&self) -> usize {
        self.state.lock().unwrap().exchanges
    }
}

async fn github_hub(github: &FakeGitHub, configure: impl FnOnce(&mut Config)) -> TestHub {
    let url = github.url.clone();
    start_hub(move |config| {
        config.github_client_id = Some(CLIENT_ID.to_string());
        config.github_client_secret = Some(Secret::new(SECRET));
        config.github_api_url = url.clone();
        config.github_web_url = url;
        configure(config);
    })
    .await
}

struct Hop {
    status: u16,
    location: String,
    cookies: Vec<String>,
    seen: String,
}

fn cookie_pair(cookies: &[String], name: &str) -> Option<String> {
    cookies
        .iter()
        .find(|cookie| cookie.starts_with(&format!("{name}=")))
        .map(|cookie| cookie.split(';').next().unwrap().to_string())
        .filter(|pair| !pair.ends_with('='))
}

struct GitHubBrowser {
    hub: String,
    state: Option<String>,
    session: Option<String>,
    http: reqwest::Client,
    seen: String,
}

impl GitHubBrowser {
    fn new(test: &TestHub) -> GitHubBrowser {
        GitHubBrowser {
            hub: test.url.clone(),
            state: None,
            session: None,
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            seen: String::new(),
        }
    }

    fn signed_in_as(test: &TestHub, person: &Browser) -> GitHubBrowser {
        GitHubBrowser {
            session: person.cookie.clone(),
            ..GitHubBrowser::new(test)
        }
    }

    async fn visit(&mut self, path: &str) -> Hop {
        let cookies: Vec<String> = [self.state.clone(), self.session.clone()]
            .into_iter()
            .flatten()
            .collect();
        let mut request = self.http.get(format!("{}{path}", self.hub));
        if !cookies.is_empty() {
            request = request.header("cookie", cookies.join("; "));
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let location = response
            .headers()
            .get("location")
            .map(|value| value.to_str().unwrap().to_string())
            .unwrap_or_default();
        let set: Vec<String> = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .map(|value| value.to_str().unwrap().to_string())
            .collect();
        let headers = format!("{:?}", response.headers());
        let body = response.text().await.unwrap();
        let seen = format!("{headers}\n{body}");
        self.seen.push_str(&seen);
        if set.iter().any(|cookie| cookie.starts_with("kebbie_github=")) {
            self.state = cookie_pair(&set, "kebbie_github");
        }
        if let Some(session) = cookie_pair(&set, "kebbie_session") {
            self.session = Some(session);
        }
        Hop {
            status,
            location,
            cookies: set,
            seen,
        }
    }

    async fn start(&mut self, query: &str) -> Hop {
        let hop = self.visit(&format!("/auth/github{query}")).await;
        assert_eq!(hop.status, 303, "{}", hop.seen);
        hop
    }

    async fn come_back(&mut self, code: &str, state: &str) -> Hop {
        let hop = self
            .visit(&format!("/auth/github/callback?code={code}&state={state}"))
            .await;
        assert_eq!(hop.status, 303, "{}", hop.seen);
        hop
    }

    async fn through_github(&mut self, github: &FakeGitHub, query: &str, id: u64, login: &str) -> Hop {
        let started = self.start(query).await;
        let (code, state) = github.approve(&started.location, id, login);
        self.come_back(&code, &state).await
    }

    fn browser(&self, test: &TestHub) -> Browser {
        let mut person = browser(test);
        person.cookie = self.session.clone();
        person
    }
}

#[tokio::test]
async fn a_github_account_signs_in_by_its_id_with_the_session_cookie_rules() {
    let github = fake_github().await;
    let test = github_hub(&github, |_| {}).await;
    let mut member = member(&test).await;
    let mut linking = GitHubBrowser::signed_in_as(&test, &member);
    let linked = linking
        .through_github(&github, "?link=1&next=/account", 4242, "octo-cat")
        .await;
    assert_eq!(linked.location, "/account");
    assert_eq!(cookie_pair(&linked.cookies, "kebbie_session"), None, "linking keeps the session");
    let view = member.account().await.unwrap();
    assert_eq!(view.github.as_deref(), Some("octo-cat"));

    let mut elsewhere = GitHubBrowser::new(&test);
    let started = elsewhere.start("").await;
    let state_cookie = started
        .cookies
        .iter()
        .find(|cookie| cookie.starts_with("kebbie_github="))
        .unwrap();
    assert!(state_cookie.contains("HttpOnly") && state_cookie.contains("SameSite=Lax"), "{state_cookie}");
    assert!(state_cookie.contains("Max-Age=300"), "{state_cookie}");
    let (code, state) = github.approve(&started.location, 4242, "octocat-renamed");
    let back = elsewhere.come_back(&code, &state).await;
    assert_eq!(back.location, "/");
    let session = back
        .cookies
        .iter()
        .find(|cookie| cookie.starts_with("kebbie_session="))
        .expect("a session cookie");
    assert!(
        session.contains("HttpOnly") && session.contains("SameSite=Strict") && session.contains("Path=/"),
        "{session}"
    );
    assert!(
        back.cookies.iter().any(|cookie| cookie.starts_with("kebbie_github=;") && cookie.contains("Max-Age=0")),
        "the state cookie is cleared"
    );
    let signed_in: AccountView = elsewhere.browser(&test).account().await.unwrap();
    assert_eq!(signed_in.account, view.account);
    assert_eq!(signed_in.github.as_deref(), Some("octocat-renamed"));
}

#[tokio::test]
async fn github_signs_up_from_a_pending_connect_and_a_passkey_then_makes_the_key_for_the_handoff() {
    let github = fake_github().await;
    let test = github_hub(&github, |config| config.signup = SignupMode::Open).await;
    let pending = kebbie_hub_fakes::machine::connect(&test.url, "mac").await.unwrap();
    let code = pending.started.user_code.clone();
    let next = format!("/connect?code={code}");
    let mut arriving = GitHubBrowser::new(&test);
    let back = arriving
        .through_github(&github, &format!("?connect={code}&next={}", urlencode(&next)), 77, "newcomer")
        .await;
    assert_eq!(back.location, next);
    let mut person = arriving.browser(&test);
    let created = person.account().await.unwrap();
    assert!(!created.owner);
    assert_eq!(created.github.as_deref(), Some("newcomer"));
    assert_eq!(created_via(&test, &created.account).as_deref(), Some("connect"));
    assert_eq!(created.key, None, "GitHub alone cannot make the account key");
    assert_eq!(created.email, None);

    let mut second = GitHubBrowser::new(&test);
    let refused = second
        .through_github(&github, &format!("?connect={code}&next={}", urlencode(&next)), 78, "late")
        .await;
    assert_eq!(refused.location, format!("{next}&github_error=unauthorized"), "one account per code");
    assert_eq!(second.session, None);

    let added = person.add_passkey().await.unwrap();
    assert!(!added.unlocks);
    person
        .log_in_with_credential(&URL_SAFE_NO_PAD.decode(&added.passkey).unwrap())
        .await
        .unwrap();
    let keyed = person.account().await.unwrap();
    assert!(keyed.key.is_some(), "the first passkey made the account key");

    let approval = person.approve_connect(&pending.link(), true).await.unwrap();
    let (mut machine, pairing, secret) = pending.wait(WAIT).await.unwrap();
    let pairing = pairing.expect("the approval opened a handoff");
    let (pinned, confirmed) = tokio::join!(
        machine.finish_connect(&pairing, &secret, WAIT),
        person.hand_off(approval, WAIT)
    );
    pinned.unwrap();
    assert_eq!(confirmed.unwrap(), machine.machine);
}

fn key_setup(account: &str, credential: &[u8]) -> AccountKeySetup {
    let secret = SecretKeys::generate();
    let key = AccountKey::new(Id::parse(account).unwrap(), 1, secret.public()).unwrap();
    let wrapped = Unlock {
        account: &key.account,
        credential,
        version: 1,
        prf_output: &[4; 32],
        salt: &[5; 32],
    }
    .wrap(&secret)
    .unwrap();
    let signature = sign_wrapper(&secret, &key, credential, &wrapped).unwrap();
    AccountKeySetup {
        key,
        credential: URL_SAFE_NO_PAD.encode(credential),
        wrapped: URL_SAFE_NO_PAD.encode(&wrapped),
        signature: Bytes64(signature),
    }
}

#[tokio::test]
async fn a_passkey_added_long_after_a_github_sign_in_still_makes_the_account_key() {
    let github = fake_github().await;
    let test = github_hub(&github, |config| config.fresh_session_ttl = 1).await;
    let invite = test.hub.mint_invite().unwrap();
    let mut arriving = GitHubBrowser::new(&test);
    arriving
        .through_github(&github, &format!("?invite={invite}"), 314, "late-passkey")
        .await;
    let mut person = arriving.browser(&test);
    let account = person.account().await.unwrap().account;
    tokio::time::sleep(Duration::from_millis(2100)).await;
    let stale = person
        .post::<_, Option<()>>("/v1/a/key", &key_setup(&account, b"not-mine"))
        .await;
    assert_eq!(hub_code(stale.unwrap_err()), (401, "reauth-required".to_string()));

    let added = person.add_passkey().await.unwrap();
    let credential = URL_SAFE_NO_PAD.decode(&added.passkey).unwrap();
    let setup = key_setup(&account, &credential);
    person.post::<_, Option<()>>("/v1/a/key", &setup).await.unwrap();
    assert_eq!(person.account().await.unwrap().key, Some(setup.key));
}

fn urlencode(text: &str) -> String {
    text.replace('?', "%3F").replace('=', "%3D").replace('&', "%26")
}

#[tokio::test]
async fn an_invite_hub_makes_no_github_account_without_a_ticket() {
    let github = fake_github().await;
    let test = github_hub(&github, |_| {}).await;
    let mut stranger = GitHubBrowser::new(&test);
    let refused = stranger.through_github(&github, "", 900, "stranger").await;
    assert_eq!(refused.location, "/?github_error=no-account");
    assert_eq!(stranger.session, None);
    assert_eq!(rows(&test, "accounts"), 0);

    let pending = kebbie_hub_fakes::machine::connect(&test.url, "mac").await.unwrap();
    let code = pending.started.user_code.clone();
    let closed = stranger
        .through_github(&github, &format!("?connect={code}"), 900, "stranger")
        .await;
    assert_eq!(closed.location, "/?github_error=signup-closed");
    assert_eq!(rows(&test, "accounts"), 0);

    let invite = test.hub.mint_invite().unwrap();
    let mut invited = GitHubBrowser::new(&test);
    let welcomed = invited
        .through_github(&github, &format!("?invite={invite}"), 901, "invited")
        .await;
    assert_eq!(welcomed.location, "/");
    let made = invited.browser(&test).account().await.unwrap();
    assert!(!made.owner);
    assert_eq!(created_via(&test, &made.account).as_deref(), Some("invite"));
    let mut again = GitHubBrowser::new(&test);
    let spent = again
        .through_github(&github, &format!("?invite={invite}"), 902, "again")
        .await;
    assert_eq!(spent.location, "/?github_error=unauthorized");
    assert_eq!(rows(&test, "accounts"), 1);
}

#[tokio::test]
async fn a_deactivated_github_account_is_refused_at_sign_in_and_gets_no_session() {
    let github = fake_github().await;
    let test = github_hub(&github, |_| {}).await;
    let invite = test.hub.mint_invite().unwrap();
    let mut invited = GitHubBrowser::new(&test);
    invited.through_github(&github, &format!("?invite={invite}"), 903, "leaver").await;
    let account = invited.browser(&test).account().await.unwrap().account;
    let token = test
        .hub
        .mint_admin_token("billing", kebbie_hub::store::AdminScope::Billing)
        .unwrap()
        .unwrap();
    kebbie_hub_fakes::HubClient::new(&test.url, Some(&token))
        .post_empty(&format!("/v1/admin/accounts/{account}/deactivate"), &())
        .await
        .unwrap();
    let sessions = rows(&test, "sessions");
    let mut again = GitHubBrowser::new(&test);
    let refused = again.through_github(&github, "", 903, "leaver").await;
    assert_eq!(refused.location, "/?github_error=deactivated");
    assert_eq!(again.session, None);
    assert_eq!(rows(&test, "sessions"), sessions);
}

#[tokio::test]
async fn the_setup_link_makes_the_owner_through_github_once() {
    let github = fake_github().await;
    let test = github_hub(&github, |_| {}).await;
    let setup = test.hub.setup_url().unwrap();
    let token = setup.split_once("token=").unwrap().1.to_string();
    let mut first = GitHubBrowser::new(&test);
    first.through_github(&github, &format!("?setup={token}"), 1, "owner").await;
    let made = first.browser(&test).account().await.unwrap();
    assert!(made.owner);
    assert_eq!(created_via(&test, &made.account).as_deref(), Some("setup"));
    assert_eq!(test.hub.setup_url(), None);
    let mut second = GitHubBrowser::new(&test);
    let refused = second.through_github(&github, &format!("?setup={token}"), 2, "second").await;
    assert_eq!(refused.location, "/?github_error=unauthorized");
}

#[tokio::test]
async fn a_github_state_is_spent_once_and_only_by_the_browser_it_was_given_to() {
    let github = fake_github().await;
    let test = github_hub(&github, |_| {}).await;
    let mut member = member(&test).await;
    let mut linking = GitHubBrowser::signed_in_as(&test, &member);
    linking.through_github(&github, "?link=1", 5, "five").await;

    let mut person = GitHubBrowser::new(&test);
    let started = person.start("").await;
    let (code, state) = github.approve(&started.location, 5, "five");
    let mut other = GitHubBrowser::new(&test);
    other.start("").await;
    let exchanges = github.exchanges();
    let stolen = other.come_back(&code, &state).await;
    assert_eq!(stolen.location, "/?github_error=expired");
    assert_eq!(other.session, None);
    assert_eq!(github.exchanges(), exchanges, "the code was never traded");

    let started = person.start("").await;
    let (code, state) = github.approve(&started.location, 5, "five");
    let back = person.come_back(&code, &state).await;
    assert_eq!(back.location, "/");
    assert!(person.session.is_some());
    person.session = None;
    person.state = cookie_pair(&started.cookies, "kebbie_github");
    let replayed = person.come_back(&code, &state).await;
    assert_eq!(replayed.location, "/?github_error=expired");
    assert_eq!(person.session, None);

    let missing = person.visit("/auth/github/callback?code=x").await;
    assert_eq!(missing.location, "/?github_error=expired");
    let started = person.start("?next=/connect%3Fcode%3DK7QF-M2XD").await;
    let (_, state) = github.approve(&started.location, 5, "five");
    let denied = person
        .visit(&format!("/auth/github/callback?error=access_denied&state={state}"))
        .await;
    assert_eq!(denied.location, "/connect?code=K7QF-M2XD&github_error=denied");
    assert_eq!(member.account().await.unwrap().github.as_deref(), Some("five"));
}

#[tokio::test]
async fn a_github_id_on_another_account_is_not_linked_and_github_comes_off_only_beside_a_passkey() {
    let github = fake_github().await;
    let test = github_hub(&github, |_| {}).await;
    let invite = test.hub.mint_invite().unwrap();
    let mut first = GitHubBrowser::new(&test);
    first.through_github(&github, &format!("?invite={invite}"), 42, "first").await;
    let mut first_account = first.browser(&test);

    let mut second = member(&test).await;
    let mut linking = GitHubBrowser::signed_in_as(&test, &second);
    let refused = linking.through_github(&github, "?link=1&next=/account", 42, "first").await;
    assert_eq!(refused.location, "/account?github_error=github-taken");
    assert_eq!(second.account().await.unwrap().github, None);

    let mut anonymous = GitHubBrowser::new(&test);
    let signed_out = anonymous.start("?link=1&next=/account").await;
    assert_eq!(signed_out.location, "/account?github_error=signed-out");

    assert_eq!(
        hub_code(first_account.delete("/v1/a/github").await.unwrap_err()),
        (409, "last-sign-in".to_string())
    );
    linking.through_github(&github, "?link=1", 43, "second").await;
    assert_eq!(second.account().await.unwrap().github.as_deref(), Some("second"));
    second.delete("/v1/a/github").await.unwrap();
    assert_eq!(second.account().await.unwrap().github, None);
    assert_eq!(hub_code(second.delete("/v1/a/github").await.unwrap_err()).0, 404);
    let mut gone = GitHubBrowser::new(&test);
    let no_account = gone.through_github(&github, "", 43, "second").await;
    assert_eq!(no_account.location, "/?github_error=no-account");
}

#[tokio::test]
async fn the_client_secret_and_the_access_token_stay_out_of_answers_files_and_logs() {
    let github = fake_github().await;
    let test = github_hub(&github, |config| config.signup = SignupMode::Open).await;
    let pending = kebbie_hub_fakes::machine::connect(&test.url, "mac").await.unwrap();
    let mut person = GitHubBrowser::new(&test);
    person
        .through_github(&github, &format!("?connect={}", pending.started.user_code), 9, "nine")
        .await;
    let mut again = GitHubBrowser::new(&test);
    again.through_github(&github, "", 9, "nine").await;
    let view: Value = person.browser(&test).get("/v1/a/account").await.unwrap();
    let info: Value = person.browser(&test).get("/v1/hub").await.unwrap();
    let seen = format!("{}{}{view}{info}", person.seen, again.seen);
    assert!(!seen.contains(SECRET));
    assert!(!seen.contains("gho_access"));
    assert!(!seen.contains("private@example.com"));
    let stored = database_bytes(&test);
    for needle in [SECRET, "gho_access1", "gho_access2", "private@example.com"] {
        assert!(!holds(&stored, needle.as_bytes()), "{needle} is in the data directory");
    }
    let connection = rusqlite::Connection::open(test.hub.config().database()).unwrap();
    let events: Vec<String> = connection
        .prepare("SELECT kind || ' ' || coalesce(detail, '') FROM events")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(events.iter().any(|event| event.contains("via=github")), "{events:?}");
    assert!(events.iter().all(|event| !event.contains(SECRET) && !event.contains("gho_")));
    assert!(!format!("{:?}", test.hub.config()).contains(SECRET));
}

#[tokio::test]
async fn github_sign_in_is_limited_like_every_anonymous_call_and_off_without_a_client() {
    let github = fake_github().await;
    let test = github_hub(&github, |config| {
        config.rate = RateLimit {
            burst: 4,
            per_minute: 1,
        }
    })
    .await;
    let info: HubInfo = browser(&test).get("/v1/hub").await.unwrap();
    assert!(info.github);
    let mut person = GitHubBrowser::new(&test);
    let statuses = [
        person.visit("/auth/github").await.status,
        person.visit("/auth/github").await.status,
        person.visit("/auth/github").await.status,
        person.visit("/auth/github").await.status,
        person.visit("/auth/github/callback?state=x").await.status,
    ];
    assert_eq!(statuses, [303, 303, 303, 429, 429]);

    let plain = start_hub(|_| {}).await;
    let mut person = GitHubBrowser::new(&plain);
    assert_eq!(person.visit("/auth/github").await.status, 404);
    assert_eq!(person.visit("/auth/github/callback?state=x&code=y").await.status, 404);
    let info: HubInfo = browser(&plain).get("/v1/hub").await.unwrap();
    assert!(!info.github);
}

#[tokio::test]
async fn a_client_id_without_its_secret_does_not_start() {
    let data = tempfile::tempdir().unwrap();
    let mut config = Config::new(data.path());
    config.github_client_id = Some(CLIENT_ID.to_string());
    let refused = kebbie_hub::Hub::open(config).err().expect("the hub refuses");
    assert!(refused.to_string().contains("KEBBIE_HUB_GITHUB_CLIENT_SECRET"), "{refused}");
}
