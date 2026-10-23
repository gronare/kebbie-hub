mod account;
mod auth;
mod command;
mod device;
mod error;
mod events;
mod limit;
mod machine;
mod pairing;
mod push;
mod report;
mod server;
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
use kelpie_hub_api::{MAX_WAIT_SECONDS, code_from_random};
use kelpie_hub_protocol::{Envelope, Id, Kind};
use tokio::sync::watch;
use webauthn_rs::prelude::{Url, Webauthn, WebauthnBuilder};

pub use auth::SESSION_COOKIE;
pub use error::ApiError;
use limit::Limiter;
pub use limit::{MAX_BUCKETS, RateLimit};
use push::Pusher;
pub use server::serve;
use store::{Pairing, Store};
use wake::{Wakeups, device_key, machine_key, pairing_key};
use webpush::{Vapid, check_subject};

pub const DATABASE_FILE: &str = "hub.sqlite3";
const REQUEST_BODY_LIMIT: usize = 16 * 1024;
const BOARD_BODY_LIMIT: usize = 1024 * 1024;
const COMMAND_BODY_LIMIT: usize = 64 * 1024;
pub(crate) const SWEEP_EVERY: Duration = Duration::from_secs(60);
const MAX_NAME_CHARS: usize = 64;
pub const MAX_STORED_INTEGER: u64 = i64::MAX as u64;
pub const DEV_ORIGIN: &str = "http://localhost:8470";
pub const DEV_RP_ID: &str = "localhost";
pub const DEV_VAPID_SUBJECT: &str = "https://github.com/gronare/kelpie-hub";
pub const VAPID_KEY_FILE: &str = "vapid.key";
pub const DEFAULT_REPORT_DIFFICULTY: u8 = 20;

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
    pub max_feeds_per_device: usize,
    pub feed_keep_alive: Duration,
    pub anonymous_devices: bool,
    pub command_rate: RateLimit,
    pub start_push_rate: RateLimit,
    pub max_queued: u64,
    pub max_undelivered_per_device: u64,
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
    pub report_difficulty: u8,
    pub report_challenge_ttl: u64,
    pub report_key_rate: RateLimit,
    pub report_address_rate: RateLimit,
    pub max_quarantined: u64,
    pub max_quarantined_per_key: u64,
    pub max_signal_batches_per_key: u64,
    pub max_signal_batches_per_address: u32,
    pub max_unvetted_signal_rows: u64,
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
            max_feeds_per_device: 8,
            feed_keep_alive: Duration::from_secs(15),
            anonymous_devices: false,
            command_rate: RateLimit {
                burst: 30,
                per_minute: 30,
            },
            start_push_rate: RateLimit {
                burst: 5,
                per_minute: 5,
            },
            max_queued: 1_000,
            max_undelivered_per_device: 100,
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
            report_difficulty: DEFAULT_REPORT_DIFFICULTY,
            report_challenge_ttl: 600,
            report_key_rate: RateLimit {
                burst: 5,
                per_minute: 1,
            },
            report_address_rate: RateLimit {
                burst: 20,
                per_minute: 4,
            },
            max_quarantined: 10_000,
            max_quarantined_per_key: 20,
            max_signal_batches_per_key: 4,
            max_signal_batches_per_address: 200,
            max_unvetted_signal_rows: 1_000_000,
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

pub fn mint_admin_token(store: &Store, name: &str) -> store::Result<Option<String>> {
    let token = auth::new_token(auth::ADMIN_TOKEN_PREFIX);
    Ok(store
        .add_admin_token(&auth::digest(&token), name, now())?
        .then_some(token))
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
    Push(String),
    Report(String),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::Store(error) => error.fmt(formatter),
            OpenError::RelyingParty(message)
            | OpenError::Push(message)
            | OpenError::Report(message) => formatter.write_str(message),
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
    let webauthn = WebauthnBuilder::new(rp_id, &url)
        .and_then(|builder| builder.rp_name("kelpie hub").build())
        .map_err(|error| refuse(format!("not a valid relying party: {error}")))?;
    Ok((webauthn, serialized))
}

pub struct Hub {
    store: Mutex<Store>,
    limiter: Limiter,
    command_limiter: Limiter,
    start_push_limiter: Limiter,
    notice_limiter: Limiter,
    device_push_limiter: Limiter,
    report_key_limiter: Limiter,
    report_address_limiter: Limiter,
    signal_addresses: Mutex<(u64, HashMap<String, u32>)>,
    challenge_secret: [u8; 32],
    pusher: Pusher,
    wakeups: Wakeups,
    closing: watch::Sender<bool>,
    webauthn: Webauthn,
    origin: String,
    feeds: Mutex<HashMap<Id, usize>>,
    config: Config,
}

pub struct FeedPermit {
    hub: Arc<Hub>,
    device: Id,
}

impl Drop for FeedPermit {
    fn drop(&mut self) {
        let mut feeds = self
            .hub
            .feeds
            .lock()
            .expect("the feeds lock is never poisoned");
        if let Some(count) = feeds.get_mut(&self.device) {
            *count -= 1;
            if *count == 0 {
                feeds.remove(&self.device);
            }
        }
    }
}

impl Hub {
    pub fn open(config: Config) -> Result<Arc<Hub>, OpenError> {
        let (webauthn, origin) = relying_party(&config.rp_id, &config.origin)?;
        check_subject(&config.vapid_subject).map_err(OpenError::Push)?;
        if config.report_difficulty > kelpie_hub_protocol::MAX_DIFFICULTY {
            return Err(OpenError::Report(format!(
                "a report difficulty of {} bits is more work than any client does; at most {}",
                config.report_difficulty,
                kelpie_hub_protocol::MAX_DIFFICULTY
            )));
        }
        let vapid = Vapid::load_or_create(&config.vapid_key).map_err(OpenError::Push)?;
        let pusher = Pusher::new(
            vapid,
            config.push_timeout,
            config.push_in_flight,
            config.push_pending,
        )
        .map_err(OpenError::Push)?;
        let store = Store::open(&config.database())?;
        Ok(Arc::new(Hub {
            store: Mutex::new(store),
            limiter: Limiter::new(config.rate, config.max_buckets),
            command_limiter: Limiter::new(config.command_rate, config.max_buckets),
            start_push_limiter: Limiter::new(config.start_push_rate, config.max_buckets),
            notice_limiter: Limiter::new(config.notice_rate, config.max_buckets),
            device_push_limiter: Limiter::new(config.device_push_rate, config.max_buckets),
            report_key_limiter: Limiter::new(config.report_key_rate, config.max_buckets),
            report_address_limiter: Limiter::new(config.report_address_rate, config.max_buckets),
            signal_addresses: Mutex::new((0, HashMap::new())),
            challenge_secret: auth::random(),
            pusher,
            wakeups: Wakeups::new(),
            closing: watch::Sender::new(false),
            webauthn,
            origin,
            feeds: Mutex::new(HashMap::new()),
            config,
        }))
    }

    pub fn open_feed(self: &Arc<Hub>, device: &Id) -> Option<FeedPermit> {
        let mut feeds = self.feeds.lock().expect("the feeds lock is never poisoned");
        let count = feeds.entry(device.clone()).or_insert(0);
        if *count >= self.config.max_feeds_per_device {
            return None;
        }
        *count += 1;
        Some(FeedPermit {
            hub: Arc::clone(self),
            device: device.clone(),
        })
    }

    pub fn open_feeds(&self, device: &Id) -> usize {
        self.feeds
            .lock()
            .expect("the feeds lock is never poisoned")
            .get(device)
            .copied()
            .unwrap_or(0)
    }

    pub fn webauthn(&self) -> &Webauthn {
        &self.webauthn
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }

    pub fn store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().expect("the store lock is never poisoned")
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn vapid_public_key(&self) -> &str {
        self.pusher.vapid.public_key()
    }

    pub fn pushes_in_flight(&self) -> usize {
        self.pusher.pending()
    }

    pub fn mint_join_code(&self) -> store::Result<String> {
        mint_join_code(&self.store(), self.config.join_code_ttl)
    }

    pub fn mint_admin_token(&self, name: &str) -> store::Result<Option<String>> {
        mint_admin_token(&self.store(), name)
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

    fn wake_device(&self, device: &Id) {
        self.wakeups.wake(&device_key(device));
    }

    fn wake_pairing(&self, pairing: &Pairing) {
        self.wakeups.wake(&pairing_key(&pairing.id));
        if let Some(device) = &pairing.device {
            self.wake_device(device);
        }
    }

    fn wake_devices(&self, devices: &[Id]) {
        for device in devices {
            self.wake_device(device);
        }
    }

    fn limit(&self, key: &str) -> Result<(), ApiError> {
        if self.limiter.allow(key) {
            Ok(())
        } else {
            Err(ApiError::rate_limited())
        }
    }

    fn limit_command(&self, device: &Id, kind: Kind) -> Result<(), ApiError> {
        let limiter = match kind {
            Kind::Start | Kind::Push => &self.start_push_limiter,
            _ => &self.command_limiter,
        };
        if limiter.allow(&format!("{device}:{}", kind.name())) {
            Ok(())
        } else {
            Err(ApiError::rate_limited())
        }
    }

    fn limit_anonymous(&self, address: &str) -> Result<(), ApiError> {
        self.limit(&format!("address:{address}"))
    }

    fn check_anonymous(&self, address: &str) -> Result<(), ApiError> {
        if self.limiter.exhausted(&format!("address:{address}")) {
            Err(ApiError::rate_limited())
        } else {
            Ok(())
        }
    }

    fn refuse_device(&self, address: &str) -> ApiError {
        match self.limit_anonymous(address) {
            Ok(()) => ApiError::unknown_device(),
            Err(limited) => limited,
        }
    }

    fn refuse_anonymous(&self, address: &str) -> ApiError {
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

    // Counted in memory and forgotten when the day turns, so no address is ever written down; a full table keeps only the addresses at their cap, so a flood of addresses never locks a newcomer out.
    pub fn signal_batch_allowed(&self, address: &str, day: u64) -> bool {
        let cap = self.config.max_signal_batches_per_address;
        let mut seen = self
            .signal_addresses
            .lock()
            .expect("the signal address lock is never poisoned");
        if seen.0 != day {
            *seen = (day, HashMap::new());
        }
        if let Some(batches) = seen.1.get_mut(address) {
            if *batches >= cap {
                return false;
            }
            *batches += 1;
            return true;
        }
        if seen.1.len() >= self.config.max_buckets {
            seen.1.retain(|_, batches| *batches >= cap);
        }
        seen.1.insert(address.to_string(), 1);
        true
    }

    pub fn sweep(&self) -> store::Result<store::Sweep> {
        let swept = self.store().sweep(now());
        self.limiter.forget_idle();
        self.command_limiter.forget_idle();
        self.start_push_limiter.forget_idle();
        self.notice_limiter.forget_idle();
        self.device_push_limiter.forget_idle();
        self.report_key_limiter.forget_idle();
        self.report_address_limiter.forget_idle();
        self.wakeups.forget_idle();
        swept
    }
}

async fn health() -> StatusCode {
    StatusCode::NO_CONTENT
}

pub fn router(hub: Arc<Hub>) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/v1/m/join", post(machine::join))
        .route(
            "/v1/m/board",
            put(machine::put_board).layer(DefaultBodyLimit::max(BOARD_BODY_LIMIT)),
        )
        .route("/v1/m/name", put(machine::rename))
        .route("/v1/m/inbox", get(machine::inbox))
        .route("/v1/m/results", post(command::post_result))
        .route("/v1/m/notices", post(push::post_notice))
        .route("/v1/d/push", put(push::subscribe).delete(push::unsubscribe))
        .route("/v1/push/vapid", get(push::vapid))
        .route(
            "/v1/d/commands",
            post(command::post_command).layer(DefaultBodyLimit::max(COMMAND_BODY_LIMIT)),
        )
        .route("/v1/d/results", get(command::results))
        .route("/v1/m/pairings", post(machine::open_pairing))
        .route("/v1/m/pairings/{id}", get(machine::pairing))
        .route("/v1/m/pairings/{id}/reply", post(machine::reply))
        .route("/v1/m/pairings/{id}/admit", post(machine::admit))
        .route("/v1/m/pairings/{id}/refuse", post(machine::refuse))
        .route("/v1/d/pairings", post(device::commit))
        .route("/v1/d/pairings/{id}", get(device::pairing))
        .route("/v1/d/pairings/{id}/reveal", post(device::reveal))
        .route("/v1/m/devices/{id}", delete(machine::remove_device))
        .route("/v1/d/boards", get(device::boards))
        .route("/v1/d/events", get(events::events))
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
        .route(
            "/v1/a/devices",
            get(account::devices).post(account::register_device),
        )
        .route("/v1/a/devices/{id}", delete(account::remove_device))
        .route("/v1/reports/challenge", get(report::challenge))
        .route("/v1/reports", post(report::post_report))
        .route("/v1/signals", post(report::post_signals))
        .route("/v1/admin/reports", get(report::inbox))
        .route("/v1/admin/reports/{id}/ack", post(report::ack))
        .fallback(web::asset)
        .layer(DefaultBodyLimit::max(REQUEST_BODY_LIMIT))
        .with_state(hub)
}
