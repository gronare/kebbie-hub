use std::collections::{BTreeMap, VecDeque};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::Stream;
use kelpie_hub_api::{
    BoardChanged, EVENT_BOARD, EVENT_MACHINE_REMOVED, EVENT_PAIRING, MachineRemoved,
    PairingChanged, PairingState,
};
use kelpie_hub_protocol::Id;
use serde::Serialize;
use tokio::sync::watch;

use crate::auth::DeviceAuth;
use crate::error::ApiError;
use crate::wake::{Subscription, device_key};
use crate::{FeedPermit, Hub, now};

#[derive(Default)]
struct Snapshot {
    machines: BTreeMap<Id, Option<(String, u64)>>,
    pairings: BTreeMap<String, PairingState>,
    next_expiry: Option<u64>,
}

struct Feed {
    _permit: FeedPermit,
    hub: Arc<Hub>,
    device: Id,
    subscription: Subscription,
    closing: watch::Receiver<bool>,
    seen: Snapshot,
    pending: VecDeque<Event>,
    started: bool,
}

fn event<T: Serialize>(name: &str, body: &T) -> Event {
    Event::default()
        .event(name)
        .data(serde_json::to_string(body).expect("an event body serializes"))
}

fn snapshot(hub: &Hub, device: &Id, at: u64) -> Result<Option<Snapshot>, ApiError> {
    let store = hub.store();
    if !store.device_exists(device)? {
        return Ok(None);
    }
    let machines = store
        .heads_for_device(device)?
        .into_iter()
        .map(|head| (head.machine, head.board))
        .collect();
    let mut pairings = BTreeMap::new();
    let mut next_expiry: Option<u64> = None;
    for pairing in store.pairings_of_device(device)? {
        let state = pairing.state_at(at);
        if !state.is_final() {
            next_expiry = Some(next_expiry.map_or(pairing.expires_at, |soonest| {
                soonest.min(pairing.expires_at)
            }));
        }
        pairings.insert(pairing.id, state);
    }
    Ok(Some(Snapshot {
        machines,
        pairings,
        next_expiry,
    }))
}

fn changes(seen: &Snapshot, current: &Snapshot) -> Vec<Event> {
    let mut events = Vec::new();
    for (machine, board) in &current.machines {
        if let Some((_, rev)) = board
            && seen.machines.get(machine) != Some(board)
        {
            events.push(event(
                EVENT_BOARD,
                &BoardChanged {
                    machine: machine.clone(),
                    rev: *rev,
                },
            ));
        }
    }
    for machine in seen.machines.keys() {
        if !current.machines.contains_key(machine) {
            events.push(event(
                EVENT_MACHINE_REMOVED,
                &MachineRemoved {
                    machine: machine.clone(),
                },
            ));
        }
    }
    for (pairing, state) in &current.pairings {
        if seen.pairings.get(pairing) != Some(state) {
            events.push(event(
                EVENT_PAIRING,
                &PairingChanged {
                    pairing: pairing.clone(),
                    state: *state,
                },
            ));
        }
    }
    events
}

impl Feed {
    async fn next(mut self) -> Option<(Result<Event, Infallible>, Feed)> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Some((Ok(event), self));
            }
            if self.started {
                let expiry = self.seen.next_expiry;
                let expired = async {
                    match expiry {
                        Some(at) => {
                            tokio::time::sleep(Duration::from_secs(at.saturating_sub(now()))).await
                        }
                        None => std::future::pending().await,
                    }
                };
                let closing = &mut self.closing;
                let closed = async {
                    let _ = closing.wait_for(|closing| *closing).await;
                };
                tokio::select! {
                    () = self.subscription.changed() => {}
                    () = expired => {}
                    () = closed => return None,
                }
            }
            self.started = true;
            self.subscription.mark_seen();
            let current = match snapshot(&self.hub, &self.device, now()) {
                Ok(Some(current)) => current,
                _ => return None,
            };
            self.pending.extend(changes(&self.seen, &current));
            self.seen = current;
        }
    }
}

pub async fn events(
    State(hub): State<Arc<Hub>>,
    DeviceAuth(device): DeviceAuth,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let permit = hub
        .open_feed(&device)
        .ok_or_else(|| ApiError::new(StatusCode::TOO_MANY_REQUESTS, "too-many-feeds"))?;
    let keep_alive = hub.config().feed_keep_alive;
    let feed = Feed {
        _permit: permit,
        subscription: hub.wakeups.subscribe(&device_key(&device)),
        closing: hub.closing.subscribe(),
        hub,
        device,
        seen: Snapshot::default(),
        pending: VecDeque::new(),
        started: false,
    };
    let stream = futures_util::stream::unfold(feed, Feed::next);
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(keep_alive)))
}
