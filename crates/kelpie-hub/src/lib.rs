mod auth;
mod device;
mod error;
mod limit;
mod machine;
mod pairing;
pub mod store;

use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post, put};
use kelpie_hub_api::{MAX_WAIT_SECONDS, code_from_random};
use kelpie_hub_protocol::{Envelope, Id};
use tokio::net::TcpListener;
use tokio::sync::watch;

pub use error::ApiError;
use limit::Limiter;
pub use limit::{MAX_BUCKETS, RateLimit};
use store::Store;

pub const DATABASE_FILE: &str = "hub.sqlite3";
const REQUEST_BODY_LIMIT: usize = 16 * 1024;
const BOARD_BODY_LIMIT: usize = 1024 * 1024;
const SWEEP_EVERY: Duration = Duration::from_secs(60);
pub const MAX_STORED_INTEGER: u64 = i64::MAX as u64;

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
}

impl Config {
    pub fn new(data_dir: impl Into<PathBuf>) -> Config {
        Config {
            data_dir: data_dir.into(),
            rate: RateLimit::default(),
            join_code_ttl: 600,
            pairing_ttl: 600,
            max_wait: Duration::from_secs(MAX_WAIT_SECONDS),
            trusted_proxy: None,
            max_buckets: MAX_BUCKETS,
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

pub fn mint_join_code(store: &Store, ttl: u64) -> store::Result<String> {
    let code = code_from_random(auth::random());
    store.add_join_code(&auth::digest(&code), now() + ttl)?;
    Ok(code)
}

pub struct Hub {
    store: Mutex<Store>,
    limiter: Limiter,
    pulse: watch::Sender<u64>,
    config: Config,
}

impl Hub {
    pub fn open(config: Config) -> store::Result<Arc<Hub>> {
        let store = Store::open(&config.database())?;
        Ok(Arc::new(Hub {
            store: Mutex::new(store),
            limiter: Limiter::new(config.rate, config.max_buckets),
            pulse: watch::Sender::new(0),
            config,
        }))
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

    pub fn enqueue(&self, machine: &Id, envelope: &Envelope) -> store::Result<u64> {
        let seq = self.store().enqueue(machine, &envelope.to_json(), now())?;
        self.changed();
        Ok(seq)
    }

    fn changed(&self) {
        self.pulse
            .send_modify(|count| *count = count.wrapping_add(1));
    }

    fn limit(&self, key: &str) -> Result<(), ApiError> {
        if self.limiter.allow(key) {
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

    fn refuse_anonymous(&self, address: &str) -> ApiError {
        match self.limit_anonymous(address) {
            Ok(()) => ApiError::unauthorized(),
            Err(limited) => limited,
        }
    }

    async fn wait_until<T>(
        &self,
        wait_seconds: u64,
        mut check: impl FnMut() -> Result<(T, bool), ApiError>,
    ) -> Result<T, ApiError> {
        let mut changes = self.pulse.subscribe();
        let wait = self.config.max_wait.min(Duration::from_secs(wait_seconds));
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            changes.borrow_and_update();
            let (value, ready) = check()?;
            if ready {
                return Ok(value);
            }
            match tokio::time::timeout_at(deadline, changes.changed()).await {
                Ok(Ok(())) => continue,
                _ => return Ok(check()?.0),
            }
        }
    }

    fn sweep(&self) {
        if let Err(error) = self.store().sweep(now()) {
            eprintln!("kelpie-hub: sweep failed: {error}");
        }
        self.limiter.forget_idle();
    }
}

pub fn router(hub: Arc<Hub>) -> Router {
    Router::new()
        .route("/v1/m/join", post(machine::join))
        .route(
            "/v1/m/board",
            put(machine::put_board).layer(DefaultBodyLimit::max(BOARD_BODY_LIMIT)),
        )
        .route("/v1/m/inbox", get(machine::inbox))
        .route("/v1/m/pairings", post(machine::open_pairing))
        .route("/v1/m/pairings/{id}", get(machine::pairing))
        .route("/v1/m/pairings/{id}/reply", post(machine::reply))
        .route("/v1/m/pairings/{id}/admit", post(machine::admit))
        .route("/v1/m/pairings/{id}/refuse", post(machine::refuse))
        .route("/v1/d/pairings", post(device::commit))
        .route("/v1/d/pairings/{id}", get(device::pairing))
        .route("/v1/d/pairings/{id}/reveal", post(device::reveal))
        .route("/v1/d/boards", get(device::boards))
        .layer(DefaultBodyLimit::max(REQUEST_BODY_LIMIT))
        .with_state(hub)
}

pub async fn serve(
    listener: TcpListener,
    hub: Arc<Hub>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let sweeper = Arc::clone(&hub);
    let sweeping = tokio::spawn(async move {
        let mut ticks = tokio::time::interval(SWEEP_EVERY);
        loop {
            ticks.tick().await;
            sweeper.sweep();
        }
    });
    let served = axum::serve(
        listener,
        router(hub).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await;
    sweeping.abort();
    served
}
