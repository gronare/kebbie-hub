use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use kebbie_hub::{Config, DEV_ORIGIN, Hub, serve};
use kebbie_hub_api::{
    CreationCeremony, DeviceRegistered, DeviceRequest, LoginBegin, LoginFinish, PasskeyView,
    RegisterBegin, RegisterFinish, RequestCeremony,
};
use kebbie_hub_fakes::browser::Browser;
use kebbie_hub_fakes::machine::{MachineState, connect, join};
use kebbie_hub_fakes::phone::PhoneState;
use kebbie_hub_fakes::{HubClient, HubError, PairError};
use kebbie_hub_protocol::{Board, Id};
use reqwest::{Method, RequestBuilder, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use webauthn_authenticator_rs::WebauthnAuthenticator;
use kebbie_hub_fakes::passkey::DiscoverablePasskey;
use webauthn_rs::prelude::Url;

pub const EXAMPLE_BOARD: &str = include_str!("../../../../examples/board.json");
pub const MARKER: &str = "zebra-umbrella-7f3a";
pub const PAIRING_TIMEOUT: Duration = Duration::from_secs(10);
pub const WAIT: Duration = Duration::from_secs(10);

pub struct TestHub {
    pub url: String,
    pub hub: Arc<Hub>,
    pub data: TempDir,
    _stop: oneshot::Sender<()>,
}

pub async fn start_hub(configure: impl FnOnce(&mut Config)) -> TestHub {
    let data = tempfile::tempdir().unwrap();
    let mut config = Config::new(data.path());
    configure(&mut config);
    let hub = Hub::open(config).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (stop, stopped) = oneshot::channel::<()>();
    tokio::spawn(serve(listener, Arc::clone(&hub), async {
        let _ = stopped.await;
    }));
    TestHub {
        url,
        hub,
        data,
        _stop: stop,
    }
}

pub async fn joined_machine(test: &TestHub) -> MachineState {
    let code = test.hub.mint_join_code().unwrap();
    join(&test.url, &code, "mac").await.unwrap()
}

pub fn board(marker: &str) -> Board {
    let mut board: Board = serde_json::from_str(EXAMPLE_BOARD).unwrap();
    board.items[0].summary = format!("{marker} summary");
    if let Some(question) = &mut board.items[0].question {
        question.text = format!("{marker} question");
    }
    board
}

pub async fn pair_both(
    machine: &mut MachineState,
    phone: &mut PhoneState,
    accept_if_same: bool,
) -> (Result<Id, PairError>, Result<Id, PairError>) {
    let (code_out, code_in) = oneshot::channel::<String>();
    let (digits_out, digits_in) = oneshot::channel::<String>();
    let (machine_digits_out, machine_digits_in) = oneshot::channel::<String>();
    let machine_side = machine.pair(
        PAIRING_TIMEOUT,
        move |code| {
            let _ = code_out.send(code.to_string());
        },
        async move |machine_digits: String| {
            let _ = machine_digits_out.send(machine_digits.clone());
            accept_if_same
                && digits_in
                    .await
                    .is_ok_and(|phone_digits| phone_digits == machine_digits)
        },
    );
    let phone_side = async {
        let code = code_in.await.unwrap();
        phone
            .pair(&code, PAIRING_TIMEOUT, async move |digits: String| {
                let _ = digits_out.send(digits.clone());
                machine_digits_in
                    .await
                    .map_or(true, |machine_digits| machine_digits == digits)
            })
            .await
    };
    tokio::join!(machine_side, phone_side)
}

pub fn status_of<T: std::fmt::Debug>(result: Result<T, HubError>) -> u16 {
    result.expect_err("the hub should refuse").status().unwrap()
}

pub fn origin() -> Url {
    Url::parse(DEV_ORIGIN).unwrap()
}

pub fn soft_passkey() -> WebauthnAuthenticator<DiscoverablePasskey> {
    WebauthnAuthenticator::new(DiscoverablePasskey::new(true))
}

static PEOPLE: AtomicUsize = AtomicUsize::new(0);

pub fn fresh_handle() -> String {
    format!("person-{}", PEOPLE.fetch_add(1, Ordering::Relaxed))
}

pub struct Person {
    pub url: String,
    pub handle: String,
    pub cookie: Option<String>,
    pub origin: Option<String>,
    pub passkey: WebauthnAuthenticator<DiscoverablePasskey>,
    http: reqwest::Client,
}

pub async fn body<T: DeserializeOwned>(response: Response) -> T {
    let status = response.status();
    let text = response.text().await.unwrap();
    assert!(status.is_success(), "{status}: {text}");
    serde_json::from_str(&text).unwrap()
}

pub async fn refusal(response: Response) -> (u16, String) {
    let status = response.status().as_u16();
    let error: Value = response.json().await.unwrap_or(Value::Null);
    (
        status,
        error["error"].as_str().unwrap_or_default().to_string(),
    )
}

impl Person {
    pub fn new(url: &str) -> Person {
        Person {
            url: url.to_string(),
            handle: fresh_handle(),
            cookie: None,
            origin: Some(DEV_ORIGIN.to_string()),
            passkey: soft_passkey(),
            http: reqwest::Client::new(),
        }
    }

    pub fn request(&self, method: Method, path: &str) -> RequestBuilder {
        let mut request = self.http.request(method, format!("{}{path}", self.url));
        if let Some(origin) = &self.origin {
            request = request.header("origin", origin);
        }
        if let Some(cookie) = &self.cookie {
            request = request.header("cookie", cookie);
        }
        request
    }

    pub async fn post<B: Serialize>(&self, path: &str, body: &B) -> Response {
        self.request(Method::POST, path)
            .json(body)
            .send()
            .await
            .unwrap()
    }

    pub async fn get(&self, path: &str) -> Response {
        self.request(Method::GET, path).send().await.unwrap()
    }

    pub async fn delete(&self, path: &str) -> Response {
        self.request(Method::DELETE, path).send().await.unwrap()
    }

    pub fn keep_cookie(&mut self, response: &Response) -> String {
        let set = response
            .headers()
            .get("set-cookie")
            .expect("the hub set a cookie")
            .to_str()
            .unwrap()
            .to_string();
        let pair = set.split(';').next().unwrap().trim().to_string();
        self.cookie = Some(pair).filter(|pair| !pair.ends_with('='));
        set
    }

    pub async fn begin_sign_up(&self, invite: &str) -> Response {
        self.post(
            "/v1/a/register/begin",
            &RegisterBegin {
                invite: invite.to_string(),
                handle: self.handle.clone(),
                ..RegisterBegin::default()
            },
        )
        .await
    }

    pub async fn finish_sign_up(&mut self, ceremony: CreationCeremony) -> Response {
        let credential = self
            .passkey
            .do_registration(origin(), ceremony.options)
            .unwrap();
        let response = self
            .post(
                "/v1/a/register/finish",
                &RegisterFinish {
                    ceremony: ceremony.ceremony,
                    credential,
                    name: Some("soft passkey".to_string()),
                },
            )
            .await;
        if response.status().is_success() {
            self.keep_cookie(&response);
        }
        response
    }

    pub async fn sign_up(&mut self, invite: &str) -> Response {
        let ceremony: CreationCeremony = body(self.begin_sign_up(invite).await).await;
        self.finish_sign_up(ceremony).await
    }

    pub async fn begin_log_in(&self) -> RequestCeremony {
        body(
            self.post(
                "/v1/a/login/begin",
                &LoginBegin {
                    handle: Some(self.handle.clone()),
                    credential: None,
                },
            )
            .await,
        )
        .await
    }

    pub async fn begin_discoverable_log_in(&self) -> RequestCeremony {
        body(self.post("/v1/a/login/begin", &LoginBegin::default()).await).await
    }

    pub async fn finish_log_in(&mut self, ceremony: RequestCeremony) -> Response {
        let credential = self
            .passkey
            .do_authentication(origin(), ceremony.options)
            .unwrap();
        let response = self
            .post(
                "/v1/a/login/finish",
                &LoginFinish {
                    ceremony: ceremony.ceremony,
                    credential,
                },
            )
            .await;
        if response.status().is_success() {
            self.keep_cookie(&response);
        }
        response
    }

    pub async fn log_in(&mut self) -> Response {
        let ceremony = self.begin_log_in().await;
        self.finish_log_in(ceremony).await
    }

    pub async fn add_passkey(
        &self,
        authenticator: &mut WebauthnAuthenticator<DiscoverablePasskey>,
    ) -> PasskeyView {
        let ceremony: CreationCeremony = body(self.post("/v1/a/passkeys/begin", &()).await).await;
        let credential = authenticator
            .do_registration(origin(), ceremony.options)
            .unwrap();
        body(
            self.post(
                "/v1/a/passkeys/finish",
                &RegisterFinish {
                    ceremony: ceremony.ceremony,
                    credential,
                    name: None,
                },
            )
            .await,
        )
        .await
    }

    pub async fn register_device(&self) -> DeviceRegistered {
        body(
            self.post(
                "/v1/a/devices",
                &DeviceRequest {
                    name: Some("phone".to_string()),
                },
            )
            .await,
        )
        .await
    }
}

pub async fn signed_up(test: &TestHub) -> Person {
    let mut person = Person::new(&test.url);
    let invite = test.hub.mint_invite().unwrap();
    let response = person.sign_up(&invite).await;
    assert!(response.status().is_success(), "{}", response.status());
    person
}

pub async fn phone_at(test: &TestHub, hub: &str) -> PhoneState {
    let registered = signed_up(test).await.register_device().await;
    PhoneState::registered(hub, registered.device, &registered.token)
}

pub async fn phone(test: &TestHub) -> PhoneState {
    phone_at(test, &test.url).await
}

pub fn device_client(test: &TestHub, registered: &DeviceRegistered) -> HubClient {
    HubClient::new(&test.url, Some(&registered.token))
}

pub fn database_bytes(test: &TestHub) -> Vec<u8> {
    let mut stored = Vec::new();
    for entry in std::fs::read_dir(test.data.path()).unwrap() {
        stored.extend(std::fs::read(entry.unwrap().path()).unwrap());
    }
    stored
}

pub fn holds(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

pub fn rows(test: &TestHub, table: &str) -> u64 {
    let connection = rusqlite::Connection::open(test.hub.config().database()).unwrap();
    connection
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[derive(Debug, PartialEq)]
pub enum Next {
    Event(String, Value),
    Quiet,
    Ended,
}

pub struct EventStream {
    response: Response,
    buffer: String,
}

impl EventStream {
    pub async fn open(url: &str, token: &str) -> EventStream {
        EventStream::connect(url, token, None).await
    }

    pub async fn open_after(url: &str, token: &str, last_event: u64) -> EventStream {
        EventStream::connect(url, token, Some(last_event)).await
    }

    async fn connect(url: &str, token: &str, last_event: Option<u64>) -> EventStream {
        let mut request = reqwest::Client::new()
            .get(format!("{url}/v1/d/events"))
            .bearer_auth(token);
        if let Some(last_event) = last_event {
            request = request.header("last-event-id", last_event.to_string());
        }
        EventStream::opened(request).await
    }

    pub async fn of_account(person: &Browser) -> EventStream {
        let request = reqwest::Client::new()
            .get(format!("{}/v1/a/events", person.hub))
            .header("cookie", person.cookie.clone().expect("signed in"));
        EventStream::opened(request).await
    }

    async fn opened(request: RequestBuilder) -> EventStream {
        let response = request.send().await.unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.headers()["content-type"].to_str().unwrap(),
            "text/event-stream"
        );
        EventStream {
            response,
            buffer: String::new(),
        }
    }

    fn parse(&mut self) -> Option<Next> {
        while let Some(end) = self.buffer.find("\n\n") {
            let block: String = self.buffer.drain(..end + 2).collect();
            let mut name = None;
            let mut data = String::new();
            for line in block.lines() {
                if let Some(value) = line.strip_prefix("event:") {
                    name = Some(value.trim().to_string());
                } else if let Some(value) = line.strip_prefix("data:") {
                    data.push_str(value.trim());
                }
            }
            if let Some(name) = name {
                return Some(Next::Event(name, serde_json::from_str(&data).unwrap()));
            }
        }
        None
    }

    pub async fn next(&mut self, within: Duration) -> Next {
        let deadline = tokio::time::Instant::now() + within;
        loop {
            if let Some(next) = self.parse() {
                return next;
            }
            match tokio::time::timeout_at(deadline, self.response.chunk()).await {
                Err(_) => return Next::Quiet,
                Ok(Ok(Some(chunk))) => self.buffer.push_str(&String::from_utf8_lossy(&chunk)),
                Ok(_) => return Next::Ended,
            }
        }
    }

    pub async fn until(&mut self, name: &str, within: Duration) -> Value {
        loop {
            match self.next(within).await {
                Next::Event(event, data) if event == name => return data,
                Next::Event(..) => continue,
                other => panic!("no {name} event: {other:?}"),
            }
        }
    }
}

pub fn browser(test: &TestHub) -> Browser {
    Browser::new(&test.url, DEV_ORIGIN, &fresh_handle())
}

pub async fn member(test: &TestHub) -> Browser {
    let mut person = browser(test);
    person
        .sign_up_with_invite(&test.hub.mint_invite().unwrap())
        .await
        .unwrap();
    person
}

pub async fn owner(test: &TestHub) -> Browser {
    let mut person = browser(test);
    let setup = test.hub.setup_url().expect("a fresh hub has a setup link");
    person.sign_up_as_owner(&setup).await.unwrap();
    person
}

pub async fn connected(test: &TestHub, person: &mut Browser) -> MachineState {
    let pending = connect(&test.url, "mac").await.unwrap();
    person
        .approve_connect(&pending.link(), false)
        .await
        .unwrap();
    pending.wait(WAIT).await.unwrap().0
}

pub async fn connected_with_board(test: &TestHub, person: &mut Browser) -> MachineState {
    let pending = connect(&test.url, "mac").await.unwrap();
    let approval = person.approve_connect(&pending.link(), true).await.unwrap();
    let (mut machine, pairing, secret) = pending.wait(WAIT).await.unwrap();
    let pairing = pairing.expect("the approval opened a handoff");
    let (pinned, confirmed) = tokio::join!(
        machine.finish_connect(&pairing, &secret, WAIT),
        person.hand_off(approval, WAIT)
    );
    pinned.unwrap();
    assert_eq!(confirmed.unwrap(), machine.machine);
    machine
}

pub fn hub_code(error: HubError) -> (u16, String) {
    (
        error.status().unwrap_or_default(),
        error.code().unwrap_or_default().to_string(),
    )
}
