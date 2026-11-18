use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use kebbie_hub_api::{NO_ACCOUNT, NOT_A_WORKER};
use kebbie_hub_protocol::Id;
use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use tokio::time::Instant;

use crate::auth::{MachineAuth, random};
use crate::error::ApiError;
use crate::{Gated, Hub, now};

// A frame of a link's TLS stream with room for the WebSocket's own header.
pub const MAX_LINK_MESSAGE: usize = 1024 * 1024 + 1024;
// Where a machine waits to be rung for a link, by machine id, the links an account has open, and the relay's sockets across the hub.
pub struct Relay {
    listening: Mutex<HashMap<String, (u64, mpsc::UnboundedSender<String>)>>,
    waiting: Mutex<HashMap<String, Waiting>>,
    open: Mutex<HashMap<String, usize>>,
    generation: AtomicU64,
    sockets: Arc<Semaphore>,
}

type Answered = (WebSocket, OwnedSemaphorePermit);

struct Waiting {
    to: Id,
    socket: oneshot::Sender<Answered>,
}

// Forgets an asker's waiting session however its request ends; after the worker took it, forgetting is a no-op.
struct Unanswered<'a> {
    relay: &'a Relay,
    session: String,
}

impl Drop for Unanswered<'_> {
    fn drop(&mut self) {
        self.relay.forget(&self.session);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ring {
    pub session: String,
    pub from: Id,
}

pub struct LinkPermit {
    hub: Arc<Hub>,
    account: String,
}

impl Drop for LinkPermit {
    fn drop(&mut self) {
        let mut open = self.hub.relay.open.lock().expect("the relay lock is never poisoned");
        if let Some(count) = open.get_mut(&self.account) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                open.remove(&self.account);
            }
        }
    }
}

impl Relay {
    pub fn new(max_sockets: usize) -> Relay {
        Relay { listening: Mutex::default(), waiting: Mutex::default(), open: Mutex::default(), generation: AtomicU64::new(0), sockets: Arc::new(Semaphore::new(max_sockets.max(1))) }
    }

    fn socket(&self) -> Result<OwnedSemaphorePermit, ApiError> {
        Arc::clone(&self.sockets).try_acquire_owned().map_err(|_| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "relay-full"))
    }

    pub fn open_sockets(&self, max_sockets: usize) -> usize {
        max_sockets.max(1) - self.sockets.available_permits()
    }

    pub fn waiting_count(&self) -> usize {
        self.waiting.lock().expect("the relay lock is never poisoned").len()
    }

    fn ringer(&self, machine: &Id) -> Option<mpsc::UnboundedSender<String>> {
        self.listening.lock().expect("the relay lock is never poisoned").get(machine.as_str()).map(|(_, ring)| ring.clone())
    }

    fn take(&self, session: &str, machine: &Id) -> Option<Waiting> {
        let mut waiting = self.waiting.lock().expect("the relay lock is never poisoned");
        if waiting.get(session).is_some_and(|found| &found.to == machine) { waiting.remove(session) } else { None }
    }

    fn forget(&self, session: &str) {
        self.waiting.lock().expect("the relay lock is never poisoned").remove(session);
    }

    pub fn open_links(&self, account: &str) -> usize {
        self.open.lock().expect("the relay lock is never poisoned").get(account).copied().unwrap_or(0)
    }
}

fn permit(hub: &Arc<Hub>, account: &str) -> Option<LinkPermit> {
    let mut open = hub.relay.open.lock().expect("the relay lock is never poisoned");
    let count = open.entry(account.to_string()).or_insert(0);
    if *count >= hub.config().relay_max_links {
        return None;
    }
    *count += 1;
    Some(LinkPermit { hub: Arc::clone(hub), account: account.to_string() })
}

#[derive(Debug, Deserialize)]
pub struct LinkQuery {
    #[serde(default)]
    listen: bool,
    to: Option<Id>,
    accept: Option<String>,
}

// A link between two machines of one account when no private network joins them: the worker waits with listen, the asker names it with to, the worker answers the ring with accept, and the hub passes the bytes of their TLS session between them, which it cannot read.
pub async fn link(State(hub): State<Arc<Hub>>, MachineAuth(machine): MachineAuth, Query(query): Query<LinkQuery>, upgrade: WebSocketUpgrade) -> Result<Response, ApiError> {
    let upgrade = upgrade.max_message_size(MAX_LINK_MESSAGE).max_frame_size(MAX_LINK_MESSAGE);
    let account = hub.store().machine_owner(&machine)?.ok_or(ApiError::conflict(NO_ACCOUNT))?;
    if query.accept.is_none() {
        hub.check_gate(&hub.store(), &account, Gated::Relay)?;
    }
    match (query.listen, query.to, query.accept) {
        (true, None, None) => {
            if hub.store().worker_os(&machine)?.is_none() {
                return Err(ApiError::conflict(NOT_A_WORKER));
            }
            let held = hub.relay.socket()?;
            Ok(upgrade.on_upgrade(move |socket| listen(hub, held, account, machine, socket)))
        }
        (false, Some(to), None) => {
            if to == machine || hub.store().machine_owner(&to)?.as_deref() != Some(account.as_str()) {
                return Err(ApiError::new(StatusCode::NOT_FOUND, "unknown-machine"));
            }
            let ring = hub.relay.ringer(&to).ok_or(ApiError::conflict("not-listening"))?;
            let permit = permit(&hub, &account).ok_or_else(|| ApiError::new(StatusCode::TOO_MANY_REQUESTS, "too-many-links"))?;
            let held = hub.relay.socket()?;
            let session = random::<16>().iter().map(|byte| format!("{byte:02x}")).collect::<String>();
            let (socket, answered) = oneshot::channel();
            hub.relay.waiting.lock().expect("the relay lock is never poisoned").insert(session.clone(), Waiting { to: to.clone(), socket });
            let unanswered = Unanswered { relay: &hub.relay, session: session.clone() };
            let rung = serde_json::to_string(&Ring { session, from: machine.clone() }).map_err(|_| ApiError::internal())?;
            if ring.send(rung).is_err() {
                return Err(ApiError::conflict("not-listening"));
            }
            let Ok(Ok(other)) = tokio::time::timeout(hub.config().relay_answer_wait, answered).await else {
                return Err(ApiError::conflict("no-answer"));
            };
            drop(unanswered);
            let link = Link { hub: Arc::clone(&hub), permit, held: [held, other.1], account, from: machine, to };
            Ok(upgrade.on_upgrade(move |socket| splice(link, socket, other.0)))
        }
        (false, None, Some(session)) => {
            let held = hub.relay.socket()?;
            let waiting = hub.relay.take(&session, &machine).ok_or_else(ApiError::not_found)?;
            Ok(upgrade.on_upgrade(move |socket| async move {
                let _ = waiting.socket.send((socket, held));
            }))
        }
        _ => Err(ApiError::bad_request("bad-link")),
    }
}

async fn closed(closing: &mut tokio::sync::watch::Receiver<bool>) {
    let _ = closing.wait_for(|closing| *closing).await;
}

async fn send_within(socket: &mut WebSocket, message: Message, wait: Duration) -> bool {
    matches!(tokio::time::timeout(wait, socket.send(message)).await, Ok(Ok(())))
}

fn still_on(hub: &Hub, account: &str, machines: &[&Id]) -> bool {
    let store = hub.store();
    machines.iter().all(|machine| store.machine_owner(machine).ok().flatten().as_deref() == Some(account))
}

fn every(period: Duration) -> tokio::time::Interval {
    let mut interval = tokio::time::interval_at(Instant::now() + period, period);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    interval
}

// Ends when the machine goes, leaves the account, or misses two pings in a row.
async fn listen(hub: Arc<Hub>, _held: OwnedSemaphorePermit, account: String, machine: Id, mut socket: WebSocket) {
    let generation = hub.relay.generation.fetch_add(1, Ordering::SeqCst);
    let (ring, mut rung) = mpsc::unbounded_channel();
    hub.relay.listening.lock().expect("the relay lock is never poisoned").insert(machine.to_string(), (generation, ring));
    let wait = hub.config().relay_send_wait;
    let mut closing = hub.closing.subscribe();
    let mut ping = every(hub.config().relay_ping);
    let mut check = every(hub.config().relay_check);
    let mut unanswered = 0u32;
    loop {
        tokio::select! {
            next = rung.recv() => match next {
                Some(text) => if !send_within(&mut socket, Message::Text(text.into()), wait).await { break },
                None => break,
            },
            read = socket.recv() => match read {
                Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                Some(Ok(Message::Pong(_))) => unanswered = 0,
                Some(Ok(_)) => {}
            },
            _ = ping.tick() => {
                if unanswered >= 2 || !send_within(&mut socket, Message::Ping(Vec::new().into()), wait).await {
                    break;
                }
                unanswered += 1;
            }
            _ = check.tick() => if !still_on(&hub, &account, &[&machine]) { break },
            () = closed(&mut closing) => break,
        }
    }
    let mut listening = hub.relay.listening.lock().expect("the relay lock is never poisoned");
    if listening.get(machine.as_str()).is_some_and(|(kept, _)| *kept == generation) {
        listening.remove(machine.as_str());
    }
}

struct Link {
    hub: Arc<Hub>,
    permit: LinkPermit,
    held: [OwnedSemaphorePermit; 2],
    account: String,
    from: Id,
    to: Id,
}

// Binary messages only, both ways, until one side closes or stalls, sends text, either machine leaves the account, the link carried its byte limit, or no binary message passed for the idle time.
async fn splice(link: Link, mut asker: WebSocket, mut worker: WebSocket) {
    let Link { hub, permit, held, account, from, to } = link;
    let (limit, idle, wait) = (hub.config().relay_max_bytes, hub.config().relay_idle, hub.config().relay_send_wait);
    let mut closing = hub.closing.subscribe();
    let mut check = every(hub.config().relay_check);
    let quiet = tokio::time::sleep(idle);
    tokio::pin!(quiet);
    let mut carried = 0u64;
    let why = loop {
        let (from_asker, read) = tokio::select! {
            read = asker.recv() => (true, read),
            read = worker.recv() => (false, read),
            () = &mut quiet => break "idle",
            _ = check.tick() => if still_on(&hub, &account, &[&from, &to]) { continue } else { break "removed" },
            () = closed(&mut closing) => break "hub-closing",
        };
        let bytes = match read {
            Some(Ok(Message::Binary(bytes))) => bytes,
            Some(Ok(Message::Text(_))) => break "text",
            Some(Ok(Message::Close(_)) | Err(_)) | None => break "closed",
            Some(Ok(_)) => continue,
        };
        carried += bytes.len() as u64;
        if carried > limit {
            break "byte-limit";
        }
        quiet.as_mut().reset(Instant::now() + idle);
        let target = if from_asker { &mut worker } else { &mut asker };
        if !send_within(target, Message::Binary(bytes), wait).await {
            break "stalled";
        }
    };
    drop(permit);
    let _ = send_within(&mut asker, Message::Close(None), wait).await;
    let _ = send_within(&mut worker, Message::Close(None), wait).await;
    drop(held);
    let _ = hub.store().log(now(), "link.relayed", Some(&to), Some(&format!("from={from} bytes={carried} ended={why}")));
}
