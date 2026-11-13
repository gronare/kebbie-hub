mod account;
mod admin;
mod auth;
mod blobs;
mod boards;
pub mod cli;
mod command;
mod connect;
mod devices;
mod error;
mod events;
mod github;
mod limit;
mod machine;
mod pairing;
mod push;
mod server;
mod signin;
pub mod store;
mod wake;
pub mod web;
pub mod webpush;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::StatusCode;
use axum::routing::{delete, get, post, put};
use kebbie_hub_api::{MAX_WAIT_SECONDS, SignupMode, code_from_random};
use kebbie_hub_protocol::{Envelope, Id, Kind, MAX_BLOB_BYTES};
use tokio::sync::watch;
use webauthn_rs::prelude::{Url, Webauthn, WebauthnBuilder};

pub use admin::{AccountSwitch, set_account_active};
pub use auth::{AccountAuth, Anonymous, DEV_SESSION_COOKIE, FreshAccount, KeyAccount, SameOrigin, SESSION_COOKIE, SessionAuth, client_address, digest, new_token, passkey_required};
pub use account::account_view;
pub use signin::{KeylessSignIn, sign_in_without_key, sign_up_without_key};
pub use error::ApiError;
pub use github::{GITHUB_API_URL, GITHUB_WEB_URL, Secret};
use github::GitHub;
use limit::Limiter;
pub use limit::{MAX_BUCKETS, RateLimit};
pub use server::{serve, serve_with};
use push::Pusher;
use store::{Pairing, Store};
use webpush::{Vapid, check_subject};
use wake::{Wakeups, account_key, machine_key, pairing_key};

pub const DATABASE_FILE: &str = "hub.sqlite3";
const REQUEST_BODY_LIMIT: usize = 16 * 1024;
const BOARD_BODY_LIMIT: usize = 1024 * 1024;
const COMMAND_BODY_LIMIT: usize = 64 * 1024;
pub(crate) const SWEEP_EVERY: Duration = Duration::from_secs(60);
const MAX_NAME_CHARS: usize = 64;
pub const MAX_STORED_INTEGER: u64 = i64::MAX as u64;
pub const DEV_ORIGIN: &str = "http://localhost:8470";
pub const DEV_RP_ID: &str = "localhost";
pub const DEV_VAPID_SUBJECT: &str = "https://github.com/gronare/kebbie-hub";
pub const VAPID_KEY_FILE: &str = "vapid.key";
const SETUP_TTL: u64 = 30 * 86_400;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Refusal {
    pub url: Option<String>,
}

// Called with the store locked, so a gate reads through the Store it is given and never calls Hub::store.
pub type CommandGate = Box<dyn Fn(&Store, &str) -> Result<(), Refusal> + Send + Sync>;

// Set by an embedder that keeps account email and serves /v1/a/email; called with the store locked, an account and the verified primary address GitHub gave.
pub type AccountEmailHook = Box<dyn Fn(&Store, &str, &str) -> store::Result<()> + Send + Sync>;

#[derive(Default)]
pub struct Hooks {
    pub command_gate: Option<CommandGate>,
    pub account_email: Option<AccountEmailHook>,
    // Set by an embedder that serves its own sign-in routes on the keyless-session seam; /v1/hub advertises it and a connect sign-up then needs one of those routes or GitHub, never a bare passkey.
    pub email_sign_in: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrustedProxy {
    Cloudflare,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub data_dir: PathBuf,
    pub rate: RateLimit,
    pub join_code_ttl: u64,
    pub pairing_ttl: u64,
    pub max_wait: Duration,
    pub trusted_proxy: Option<TrustedProxy>,
    pub max_buckets: usize,
    pub rp_id: String,
    pub origin: String,
    pub session_ttl: u64,
    pub invite_ttl: u64,
    pub ceremony_ttl: u64,
    pub header_read_timeout: Duration,
    pub body_frame_timeout: Duration,
    pub max_connections: usize,
    pub max_feeds_per_account: usize,
    pub feed_keep_alive: Duration,
    pub command_rate: RateLimit,
    pub start_push_rate: RateLimit,
    pub max_queued: u64,
    pub max_undelivered_per_account: u64,
    pub max_devices_per_account: u64,
    pub signup: SignupMode,
    pub commands: bool,
    pub github_client_id: Option<String>,
    pub github_client_secret: Option<Secret>,
    pub github_api_url: String,
    pub github_web_url: String,
    pub fresh_session_ttl: u64,
    pub connect_ttl: u64,
    pub connect_interval: u64,
    pub connect_rate: RateLimit,
    pub code_rate: RateLimit,
    pub vapid_subject: String,
    pub vapid_key: PathBuf,
    pub push_allow_hosts: Vec<String>,
    pub notice_rate: RateLimit,
    pub device_push_rate: RateLimit,
    pub push_in_flight: usize,
    pub push_pending: usize,
    pub push_retries: u32,
    pub push_backoff: Duration,
    pub push_timeout: Duration,
}

impl Config {
    pub fn new(data_dir: impl Into<PathBuf>) -> Config {
        let data_dir = data_dir.into();
        Config {
            vapid_key: data_dir.join(VAPID_KEY_FILE),
            data_dir,
            rate: RateLimit::default(),
            join_code_ttl: 600,
            pairing_ttl: 600,
            max_wait: Duration::from_secs(MAX_WAIT_SECONDS),
            trusted_proxy: None,
            max_buckets: MAX_BUCKETS,
            rp_id: DEV_RP_ID.to_string(),
            origin: DEV_ORIGIN.to_string(),
            session_ttl: 14 * 86_400,
            invite_ttl: 7 * 86_400,
            ceremony_ttl: 300,
            header_read_timeout: Duration::from_secs(10),
            body_frame_timeout: Duration::from_secs(30),
            max_connections: 4096,
            max_feeds_per_account: 8,
            feed_keep_alive: Duration::from_secs(15),
            command_rate: RateLimit {
                burst: 30,
                per_minute: 30,
            },
            start_push_rate: RateLimit {
                burst: 5,
                per_minute: 5,
            },
            max_queued: 1_000,
            max_undelivered_per_account: 100,
            max_devices_per_account: 20,
            signup: SignupMode::Invite,
            commands: false,
            github_client_id: None,
            github_client_secret: None,
            github_api_url: GITHUB_API_URL.to_string(),
            github_web_url: GITHUB_WEB_URL.to_string(),
            fresh_session_ttl: 300,
            connect_ttl: 600,
            connect_interval: 5,
            connect_rate: RateLimit {
                burst: 10,
                per_minute: 10,
            },
            code_rate: RateLimit {
                burst: 10,
                per_minute: 10,
            },
            vapid_subject: DEV_VAPID_SUBJECT.to_string(),
            push_allow_hosts: Vec::new(),
            notice_rate: RateLimit {
                burst: 30,
                per_minute: 30,
            },
            device_push_rate: RateLimit {
                burst: 30,
                per_minute: 30,
            },
            push_in_flight: 256,
            push_pending: 4096,
            push_retries: 3,
            push_backoff: Duration::from_secs(1),
            push_timeout: Duration::from_secs(30),
        }
    }

    pub fn database(&self) -> PathBuf {
        database_path(&self.data_dir)
    }
}

pub fn database_path(data_dir: &Path) -> PathBuf {
    data_dir.join(DATABASE_FILE)
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

pub fn clean_name(text: &str) -> Option<&str> {
    let name = text.trim();
    let valid = !name.is_empty()
        && name.chars().count() <= MAX_NAME_CHARS
        && !name.chars().any(char::is_control);
    valid.then_some(name)
}

pub fn mint_join_code(store: &Store, ttl: u64) -> store::Result<String> {
    let code = code_from_random(auth::random());
    store.add_join_code(&auth::digest(&code), now() + ttl)?;
    Ok(code)
}

pub fn mint_invite(store: &Store, ttl: u64) -> store::Result<String> {
    let code = code_from_random(auth::random());
    store.add_invite(&auth::digest(&code), now() + ttl)?;
    Ok(code)
}

#[derive(Debug)]
pub enum OpenError {
    Store(rusqlite::Error),
    RelyingParty(String),
    DataDir(String),
    GitHub(String),
    Push(String),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::Store(error) => error.fmt(formatter),
            OpenError::RelyingParty(message)
            | OpenError::DataDir(message)
            | OpenError::GitHub(message)
            | OpenError::Push(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for OpenError {}

impl From<rusqlite::Error> for OpenError {
    fn from(error: rusqlite::Error) -> OpenError {
        OpenError::Store(error)
    }
}

pub fn relying_party(rp_id: &str, origin: &str) -> Result<(Webauthn, String), OpenError> {
    let refuse = |what: String| {
        OpenError::RelyingParty(format!("{what} (rp id {rp_id:?}, origin {origin:?})"))
    };
    let url = Url::parse(origin)
        .map_err(|error| refuse(format!("the origin does not parse: {error}")))?;
    let serialized = url.origin().ascii_serialization();
    if serialized.trim_end_matches('/') != origin.trim_end_matches('/') {
        return Err(refuse(format!(
            "the origin must be scheme, host and port only, like {serialized}"
        )));
    }
    if url.scheme() != "https" && !(url.scheme() == "http" && is_loopback(&url)) {
        return Err(refuse(
            "the origin must be https, or plain http on localhost or a loopback address".to_string(),
        ));
    }
    let webauthn = WebauthnBuilder::new(rp_id, &url)
        .and_then(|builder| builder.rp_name("kebbie hub").build())
        .map_err(|error| refuse(format!("not a valid relying party: {error}")))?;
    Ok((webauthn, serialized))
}

fn is_loopback(url: &Url) -> bool {
    let host = url.host_str().unwrap_or_default();
    host == "localhost"
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

pub struct Hub {
    store: Mutex<Store>,
    limiter: Limiter,
    command_limiter: Limiter,
    start_push_limiter: Limiter,
    connect_limiter: Limiter,
    code_limiter: Limiter,
    notice_limiter: Limiter,
    device_push_limiter: Limiter,
    pusher: Pusher,
    setup_url: Mutex<Option<String>>,
    wakeups: Wakeups,
    closing: watch::Sender<bool>,
    webauthn: Webauthn,
    github: Option<GitHub>,
    origin: String,
    feeds: Mutex<HashMap<String, usize>>,
    command_gate: Option<CommandGate>,
    account_email: Option<AccountEmailHook>,
    email_sign_in: bool,
    config: Config,
}

pub struct FeedPermit {
    hub: Arc<Hub>,
    key: String,
}

impl Drop for FeedPermit {
    fn drop(&mut self) {
        let mut feeds = self
            .hub
            .feeds
            .lock()
            .expect("the feeds lock is never poisoned");
        if let Some(count) = feeds.get_mut(&self.key) {
            *count -= 1;
            if *count == 0 {
                feeds.remove(&self.key);
            }
        }
    }
}

impl Hub {
    pub fn open(config: Config) -> Result<Arc<Hub>, OpenError> {
        Hub::open_with(config, None)
    }

    pub fn open_with(config: Config, command_gate: Option<CommandGate>) -> Result<Arc<Hub>, OpenError> {
        Hub::open_with_hooks(config, Hooks { command_gate, ..Hooks::default() })
    }

    pub fn open_with_hooks(config: Config, hooks: Hooks) -> Result<Arc<Hub>, OpenError> {
        let (webauthn, origin) = relying_party(&config.rp_id, &config.origin)?;
        std::fs::create_dir_all(&config.data_dir)
            .map_err(|error| OpenError::DataDir(format!("{}: {error}", config.data_dir.display())))?;
        let github = match (&config.github_client_id, &config.github_client_secret) {
            (Some(id), Some(secret)) => Some(
                GitHub::new(id, secret.clone(), &config.github_api_url, &config.github_web_url)
                    .map_err(OpenError::GitHub)?,
            ),
            (Some(_), None) => {
                return Err(OpenError::GitHub(
                    "a GitHub client id needs its client secret in KEBBIE_HUB_GITHUB_CLIENT_SECRET".to_string(),
                ));
            }
            (None, _) => None,
        };
        check_subject(&config.vapid_subject).map_err(OpenError::Push)?;
        let vapid = Vapid::load_or_create(&config.vapid_key).map_err(OpenError::Push)?;
        let pusher = Pusher::new(vapid, config.push_timeout, config.push_in_flight, config.push_pending)
            .map_err(OpenError::Push)?;
        let store = Store::open(&config.database())?;
        let setup_url = if store.account_count()? == 0 {
            let token = auth::new_token(auth::SETUP_TOKEN_PREFIX);
            store.replace_setup_ticket(&auth::digest(&token), now() + SETUP_TTL)?;
            Some(format!("{origin}/setup?token={token}"))
        } else {
            None
        };
        Ok(Arc::new(Hub {
            connect_limiter: Limiter::new(config.connect_rate, config.max_buckets),
            code_limiter: Limiter::new(config.code_rate, config.max_buckets),
            notice_limiter: Limiter::new(config.notice_rate, config.max_buckets),
            device_push_limiter: Limiter::new(config.device_push_rate, config.max_buckets),
            pusher,
            setup_url: Mutex::new(setup_url),
            store: Mutex::new(store),
            limiter: Limiter::new(config.rate, config.max_buckets),
            command_limiter: Limiter::new(config.command_rate, config.max_buckets),
            start_push_limiter: Limiter::new(config.start_push_rate, config.max_buckets),
            wakeups: Wakeups::new(),
            closing: watch::Sender::new(false),
            webauthn,
            github,
            origin,
            feeds: Mutex::new(HashMap::new()),
            command_gate: hooks.command_gate,
            account_email: hooks.account_email,
            email_sign_in: hooks.email_sign_in,
            config,
        }))
    }

    pub fn open_feed(self: &Arc<Hub>, key: &str) -> Option<FeedPermit> {
        let mut feeds = self.feeds.lock().expect("the feeds lock is never poisoned");
        let count = feeds.entry(key.to_string()).or_insert(0);
        if *count >= self.config.max_feeds_per_account {
            return None;
        }
        *count += 1;
        Some(FeedPermit {
            hub: Arc::clone(self),
            key: key.to_string(),
        })
    }

    pub fn open_feeds(&self, account: &str) -> usize {
        self.feeds
            .lock()
            .expect("the feeds lock is never poisoned")
            .get(&account_key(account))
            .copied()
            .unwrap_or(0)
    }

    pub fn webauthn(&self) -> &Webauthn {
        &self.webauthn
    }

    fn github(&self) -> Option<&GitHub> {
        self.github.as_ref()
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }

    pub fn plain_http(&self) -> bool {
        self.origin.starts_with("http://")
    }

    pub fn session_cookie_name(&self) -> &'static str {
        if self.plain_http() {
            auth::DEV_SESSION_COOKIE
        } else {
            auth::SESSION_COOKIE
        }
    }

    pub fn store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().expect("the store lock is never poisoned")
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn mint_join_code(&self) -> store::Result<String> {
        mint_join_code(&self.store(), self.config.join_code_ttl)
    }

    pub fn setup_url(&self) -> Option<String> {
        self.setup_url
            .lock()
            .expect("the setup lock is never poisoned")
            .clone()
    }

    fn forget_setup(&self) {
        *self
            .setup_url
            .lock()
            .expect("the setup lock is never poisoned") = None;
    }

    pub fn vapid_public_key(&self) -> &str {
        self.pusher.vapid.public_key()
    }

    pub fn pushes_in_flight(&self) -> usize {
        self.pusher.pending()
    }

    pub fn email_sign_in(&self) -> bool {
        self.email_sign_in
    }

    pub fn keeps_email(&self) -> bool {
        self.account_email.is_some()
    }

    pub(crate) fn verified_email(&self, account: &str, email: &str) {
        let Some(hook) = &self.account_email else {
            return;
        };
        if let Err(error) = hook(&self.store(), account, email) {
            eprintln!("kebbie-hub: keeping {account}'s verified email failed: {error}");
        }
    }

    pub fn has_command_gate(&self) -> bool {
        self.command_gate.is_some()
    }

    pub(crate) fn check_command_gate(&self, store: &Store, machine: &Id) -> Result<(), ApiError> {
        match store.machine_owner(machine)? {
            Some(account) => self.check_account_gate(store, &account),
            None => Ok(()),
        }
    }

    pub(crate) fn check_account_gate(&self, store: &Store, account: &str) -> Result<(), ApiError> {
        let Some(gate) = &self.command_gate else {
            return Ok(());
        };
        gate(store, account).map_err(|refusal| {
            ApiError::new(StatusCode::PAYMENT_REQUIRED, kebbie_hub_api::PLAN_INACTIVE).with_url(refusal.url)
        })
    }

    pub fn mint_invite(&self) -> store::Result<String> {
        mint_invite(&self.store(), self.config.invite_ttl)
    }

    pub fn enqueue(&self, machine: &Id, envelope: &Envelope) -> store::Result<u64> {
        let seq = self
            .store()
            .enqueue(machine, &envelope.to_json(), now(), envelope.header.exp)?;
        self.wakeups.wake(&machine_key(machine));
        Ok(seq)
    }

    fn wake_pairing(&self, pairing: &Pairing) {
        self.wakeups.wake(&pairing_key(&pairing.id));
    }

    fn wake_account(&self, account: &str) {
        self.wakeups.wake(&account_key(account));
    }

    fn limit(&self, key: &str) -> Result<(), ApiError> {
        if self.limiter.allow(key) {
            Ok(())
        } else {
            Err(ApiError::rate_limited())
        }
    }

    fn limit_command(&self, account: &str, kind: Kind) -> Result<(), ApiError> {
        let limiter = match kind {
            Kind::Start | Kind::StartItem | Kind::FoundStart | Kind::Push | Kind::AcceptStep | Kind::AcceptSubtask => &self.start_push_limiter,
            _ => &self.command_limiter,
        };
        if limiter.allow(&format!("{account}:{}", kind.name())) {
            Ok(())
        } else {
            Err(ApiError::rate_limited())
        }
    }

    pub fn limit_anonymous(&self, address: &str) -> Result<(), ApiError> {
        self.limit(&format!("address:{address}"))
    }

    fn check_anonymous(&self, address: &str) -> Result<(), ApiError> {
        if self.limiter.exhausted(&format!("address:{address}")) {
            Err(ApiError::rate_limited())
        } else {
            Ok(())
        }
    }

    pub fn refuse_anonymous(&self, address: &str) -> ApiError {
        match self.limit_anonymous(address) {
            Ok(()) => ApiError::unauthorized(),
            Err(limited) => limited,
        }
    }

    async fn wait_until<T>(
        &self,
        key: &str,
        wait_seconds: u64,
        mut check: impl FnMut() -> Result<(T, bool), ApiError>,
    ) -> Result<T, ApiError> {
        let mut changes = self.wakeups.subscribe(key);
        let mut closing = self.closing.subscribe();
        let wait = self.config.max_wait.min(Duration::from_secs(wait_seconds));
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            changes.mark_seen();
            let (value, ready) = check()?;
            if ready || *closing.borrow() {
                return Ok(value);
            }
            let closed = async {
                let _ = closing.wait_for(|closing| *closing).await;
            };
            tokio::select! {
                () = changes.changed() => {}
                () = closed => return Ok(check()?.0),
                () = tokio::time::sleep_until(deadline) => return Ok(check()?.0),
            }
        }
    }

    pub fn sweep(&self) -> store::Result<store::Sweep> {
        let swept = self.store().sweep(now());
        self.limiter.forget_idle();
        self.command_limiter.forget_idle();
        self.start_push_limiter.forget_idle();
        self.connect_limiter.forget_idle();
        self.code_limiter.forget_idle();
        self.notice_limiter.forget_idle();
        self.device_push_limiter.forget_idle();
        self.wakeups.forget_idle();
        swept
    }
}

async fn health() -> StatusCode {
    StatusCode::NO_CONTENT
}

pub fn router(hub: Arc<Hub>) -> Router {
    router_with(hub, Router::new())
}

pub fn router_with(hub: Arc<Hub>, extra: Router<Arc<Hub>>) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/v1/hub", get(account::hub_info))
        .route("/auth/github", get(github::start))
        .route("/auth/github/callback", get(github::callback))
        .route("/v1/a/github", delete(account::remove_github))
        .route("/v1/m/join", post(machine::join))
        .route("/v1/m/connect", post(connect::start))
        .route("/v1/m/connect/token", post(connect::token))
        .route("/v1/connect", get(connect::peek))
        .route("/v1/a/connect", get(connect::view))
        .route("/v1/a/connect/approve", post(connect::approve))
        .route("/v1/a/connect/deny", post(connect::deny))
        .route("/v1/a/pairings/{id}", get(connect::pairing))
        .route("/v1/a/pairings/{id}/reveal", post(connect::reveal))
        .route("/v1/a/boards", get(boards::boards))
        .route("/v1/a/machines", put(boards::put_machines))
        .route("/v1/a/machines/{id}", delete(boards::remove_machine))
        .route("/v1/a/events", get(events::account_events))
        .route("/v1/a/key", post(account::set_key))
        .route(
            "/v1/m/board",
            put(machine::put_board).layer(DefaultBodyLimit::max(BOARD_BODY_LIMIT)),
        )
        .route(
            "/v1/m/blobs/{id}",
            put(blobs::put_blob).layer(DefaultBodyLimit::max(MAX_BLOB_BYTES)).delete(blobs::delete_blob),
        )
        .route("/v1/a/blobs/{id}", get(blobs::blob))
        .route("/v1/m/name", put(machine::rename))
        .route("/v1/m/leave", post(machine::leave))
        .route("/v1/m/inbox", get(machine::inbox))
        .route("/v1/m/results", post(command::post_result))
        .route(
            "/v1/a/commands",
            post(command::post_command).layer(DefaultBodyLimit::max(COMMAND_BODY_LIMIT)),
        )
        .route("/v1/a/results", get(command::results))
        .route("/v1/a/devices", get(devices::devices).post(devices::post_device))
        .route("/v1/a/devices/{id}", delete(devices::remove_device))
        .route("/v1/a/devices/{id}/commands", get(devices::device_commands))
        .route("/v1/a/devices/{id}/push", put(push::subscribe).delete(push::unsubscribe))
        .route("/v1/a/devices/{id}/categories", put(push::set_categories))
        .route("/v1/m/devices", get(push::notified_devices))
        .route("/v1/m/notices", post(push::post_notice))
        .route("/v1/m/pairings/{id}", get(machine::pairing))
        .route("/v1/m/pairings/{id}/reply", post(machine::reply))
        .route("/v1/m/pairings/{id}/admit", post(machine::admit))
        .route("/v1/m/pairings/{id}/refuse", post(machine::refuse))
        .route("/v1/a/register/begin", post(account::register_begin))
        .route("/v1/a/register/finish", post(account::register_finish))
        .route("/v1/a/login/begin", post(account::login_begin))
        .route("/v1/a/login/finish", post(account::login_finish))
        .route("/v1/a/logout", post(account::logout))
        .route("/v1/a/account", get(account::account))
        .route("/v1/a/passkeys", get(account::passkeys))
        .route("/v1/a/passkeys/begin", post(account::passkey_begin))
        .route("/v1/a/passkeys/finish", post(account::passkey_finish))
        .route("/v1/a/passkeys/{id}", delete(account::remove_passkey))
        .route("/v1/a/passkeys/{id}/wrapper", put(account::set_wrapper))
        .merge(extra)
        .fallback(web::asset)
        .layer(DefaultBodyLimit::max(REQUEST_BODY_LIMIT))
        .with_state(hub)
}
