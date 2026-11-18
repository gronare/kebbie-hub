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
use tokio::sync::{mpsc, oneshot};

use crate::auth::{MachineAuth, random};
use crate::error::ApiError;
use crate::{Gated, Hub, now};

// A frame of a link's TLS stream with room for the WebSocket's own header.
pub const MAX_LINK_MESSAGE: usize = 1024 * 1024 + 1024;
const PING_EVERY: Duration = Duration::from_secs(30);

// Where a machine waits to be rung for a link, by machine id, and the links an account has open.
#[derive(Default)]
pub struct Relay {
    listening: Mutex<HashMap<String, (u64, mpsc::UnboundedSender<String>)>>,
    waiting: Mutex<HashMap<String, Waiting>>,
    open: Mutex<HashMap<String, usize>>,
    generation: AtomicU64,
}

struct Waiting {
    to: Id,
    socket: oneshot::Sender<WebSocket>,
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
            Ok(upgrade.on_upgrade(move |socket| listen(hub, machine, socket)))
        }
        (false, Some(to), None) => {
            if to == machine || hub.store().machine_owner(&to)?.as_deref() != Some(account.as_str()) {
                return Err(ApiError::new(StatusCode::NOT_FOUND, "unknown-machine"));
            }
            let ring = hub.relay.ringer(&to).ok_or(ApiError::conflict("not-listening"))?;
            let permit = permit(&hub, &account).ok_or_else(|| ApiError::new(StatusCode::TOO_MANY_REQUESTS, "too-many-links"))?;
            let session = random::<16>().iter().map(|byte| format!("{byte:02x}")).collect::<String>();
            let (socket, answered) = oneshot::channel();
            hub.relay.waiting.lock().expect("the relay lock is never poisoned").insert(session.clone(), Waiting { to: to.clone(), socket });
            let rung = serde_json::to_string(&Ring { session: session.clone(), from: machine.clone() }).map_err(|_| ApiError::internal())?;
            if ring.send(rung).is_err() {
                hub.relay.forget(&session);
                return Err(ApiError::conflict("not-listening"));
            }
            let other = match tokio::time::timeout(hub.config().relay_answer_wait, answered).await {
                Ok(Ok(other)) => other,
                _ => {
                    hub.relay.forget(&session);
                    return Err(ApiError::conflict("no-answer"));
                }
            };
            Ok(upgrade.on_upgrade(move |socket| splice(hub, permit, machine, to, socket, other)))
        }
        (false, None, Some(session)) => {
            let waiting = hub.relay.take(&session, &machine).ok_or_else(ApiError::not_found)?;
            Ok(upgrade.on_upgrade(move |socket| async move {
                let _ = waiting.socket.send(socket);
            }))
        }
        _ => Err(ApiError::bad_request("bad-link")),
    }
}

async fn closed(closing: &mut tokio::sync::watch::Receiver<bool>) {
    let _ = closing.wait_for(|closing| *closing).await;
}

async fn listen(hub: Arc<Hub>, machine: Id, mut socket: WebSocket) {
    let generation = hub.relay.generation.fetch_add(1, Ordering::SeqCst);
    let (ring, mut rung) = mpsc::unbounded_channel();
    hub.relay.listening.lock().expect("the relay lock is never poisoned").insert(machine.to_string(), (generation, ring));
    let mut closing = hub.closing.subscribe();
    let mut ping = tokio::time::interval(PING_EVERY);
    loop {
        tokio::select! {
            next = rung.recv() => match next {
                Some(text) => if socket.send(Message::Text(text.into())).await.is_err() { break },
                None => break,
            },
            read = socket.recv() => match read {
                Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
            _ = ping.tick() => if socket.send(Message::Ping(Vec::new().into())).await.is_err() { break },
            () = closed(&mut closing) => break,
        }
    }
    let mut listening = hub.relay.listening.lock().expect("the relay lock is never poisoned");
    if listening.get(machine.as_str()).is_some_and(|(kept, _)| *kept == generation) {
        listening.remove(machine.as_str());
    }
}

// Binary messages only, both ways, until one side closes, the link carried its byte limit or went quiet for the idle time.
async fn splice(hub: Arc<Hub>, permit: LinkPermit, from: Id, to: Id, mut asker: WebSocket, mut worker: WebSocket) {
    let (limit, idle) = (hub.config().relay_max_bytes, hub.config().relay_idle);
    let mut closing = hub.closing.subscribe();
    let mut carried = 0u64;
    let why = loop {
        let (from_asker, read) = tokio::select! {
            read = asker.recv() => (true, read),
            read = worker.recv() => (false, read),
            () = tokio::time::sleep(idle) => break "idle",
            () = closed(&mut closing) => break "hub-closing",
        };
        let bytes = match read {
            Some(Ok(Message::Binary(bytes))) => bytes,
            Some(Ok(Message::Close(_)) | Err(_)) | None => break "closed",
            Some(Ok(_)) => continue,
        };
        carried += bytes.len() as u64;
        if carried > limit {
            break "byte-limit";
        }
        let sent = if from_asker { worker.send(Message::Binary(bytes)).await } else { asker.send(Message::Binary(bytes)).await };
        if sent.is_err() {
            break "closed";
        }
    };
    let _ = asker.send(Message::Close(None)).await;
    let _ = worker.send(Message::Close(None)).await;
    let _ = hub.store().log(now(), "link.relayed", Some(&to), Some(&format!("from={from} bytes={carried} ended={why}")));
    drop(permit);
}
