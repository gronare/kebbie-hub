use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::Stream;
use kebbie_hub_api::{
    BoardChanged, EVENT_BOARD, EVENT_MACHINE_REMOVED, EVENT_MACHINES, EVENT_PAIRING, EVENT_RESULT,
    MachineRemoved, MachinesChanged, PairingChanged, PairingState, ResultReady,
};
use kebbie_hub_protocol::Id;
use serde::Serialize;
use tokio::sync::watch;

use crate::auth::{AccountAuth, DeviceAuth};
use crate::error::ApiError;
use crate::store::BoardHead;
use crate::wake::{Subscription, account_key, device_key};
use crate::{FeedPermit, Hub, MAX_STORED_INTEGER, now};

#[derive(Default)]
struct Snapshot {
    machines: BTreeSet<Id>,
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
    results_after: u64,
}

fn event<T: Serialize>(name: &str, body: &T) -> Event {
    Event::default()
        .event(name)
        .data(serde_json::to_string(body).expect("an event body serializes"))
}

fn removed(machine: &Id) -> Event {
    event(
        EVENT_MACHINE_REMOVED,
        &MachineRemoved {
            machine: machine.clone(),
        },
    )
}

fn snapshot(hub: &Hub, device: &Id, at: u64) -> Result<Option<Snapshot>, ApiError> {
    let store = hub.store();
    if !store.device_serves(device)? {
        return Ok(None);
    }
    let machines = store.admitting_machines(device)?.into_iter().collect();
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
    let mut events: Vec<Event> = seen
        .machines
        .difference(&current.machines)
        .map(removed)
        .collect();
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

async fn woken(
    subscription: &mut Subscription,
    closing: &mut watch::Receiver<bool>,
    expiry: Option<u64>,
) -> bool {
    let expired = async {
        match expiry {
            Some(at) => tokio::time::sleep(Duration::from_secs(at.saturating_sub(now()))).await,
            None => std::future::pending().await,
        }
    };
    let closed = async {
        let _ = closing.wait_for(|closing| *closing).await;
    };
    tokio::select! {
        () = subscription.changed() => true,
        () = expired => true,
        () = closed => false,
    }
}

impl Feed {
    async fn next(mut self) -> Option<(Result<Event, Infallible>, Feed)> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Some((Ok(event), self));
            }
            if self.started
                && !woken(
                    &mut self.subscription,
                    &mut self.closing,
                    self.seen.next_expiry,
                )
                .await
            {
                return None;
            }
            self.started = true;
            self.subscription.mark_seen();
            let current = match snapshot(&self.hub, &self.device, now()) {
                Ok(Some(current)) => current,
                _ => return None,
            };
            self.pending.extend(changes(&self.seen, &current));
            self.seen = current;
            let Ok(results) =
                self.hub
                    .store()
                    .result_heads_after(&self.device, self.results_after, now())
            else {
                return None;
            };
            for result in results {
                self.results_after = result.seq;
                self.pending.push_back(
                    event(
                        EVENT_RESULT,
                        &ResultReady {
                            machine: result.machine,
                            command: result.command,
                        },
                    )
                    .id(result.seq.to_string()),
                );
            }
        }
    }
}

pub async fn events(
    State(hub): State<Arc<Hub>>,
    DeviceAuth(device): DeviceAuth,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let results_after = match headers.get("last-event-id") {
        None => 0,
        Some(value) => value
            .to_str()
            .ok()
            .and_then(|text| text.trim().parse::<u64>().ok())
            .filter(|seq| *seq <= MAX_STORED_INTEGER)
            .ok_or(ApiError::bad_request("bad-last-event-id"))?,
    };
    let permit = hub
        .open_feed(&device_key(&device))
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
        results_after,
    };
    let stream = futures_util::stream::unfold(feed, Feed::next);
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(keep_alive)))
}

struct AccountFeed {
    _permit: FeedPermit,
    hub: Arc<Hub>,
    account: String,
    subscription: Subscription,
    closing: watch::Receiver<bool>,
    seen: BTreeMap<Id, BoardHead>,
    counter: Option<u64>,
    pending: VecDeque<Event>,
    started: bool,
}

fn account_changes(
    seen: &BTreeMap<Id, BoardHead>,
    current: &BTreeMap<Id, BoardHead>,
) -> Vec<Event> {
    let mut events = Vec::new();
    for (machine, head) in current {
        if let Some((_, rev)) = &head.board
            && seen.get(machine) != Some(head)
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
    events.extend(
        seen.keys()
            .filter(|machine| !current.contains_key(*machine))
            .map(removed),
    );
    events
}

impl AccountFeed {
    async fn next(mut self) -> Option<(Result<Event, Infallible>, AccountFeed)> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Some((Ok(event), self));
            }
            if self.started && !woken(&mut self.subscription, &mut self.closing, None).await {
                return None;
            }
            self.started = true;
            self.subscription.mark_seen();
            let Ok(Some(row)) = self.hub.store().account(&self.account) else {
                return None;
            };
            if row.deactivated_at.is_some() {
                return None;
            }
            let Ok(heads) = self.hub.store().heads_for_account(&self.account) else {
                return None;
            };
            let current: BTreeMap<Id, BoardHead> = heads
                .into_iter()
                .map(|head| (head.machine.clone(), head))
                .collect();
            self.pending.extend(account_changes(&self.seen, &current));
            self.seen = current;
            let Ok(list) = self.hub.store().machine_list(&self.account) else {
                return None;
            };
            let counter = list.map(|(list, _)| list.counter);
            if counter != self.counter
                && let Some(counter) = counter
            {
                self.pending
                    .push_back(event(EVENT_MACHINES, &MachinesChanged { counter }));
            }
            self.counter = counter;
        }
    }
}

pub async fn account_events(
    State(hub): State<Arc<Hub>>,
    AccountAuth(account): AccountAuth,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let permit = hub
        .open_feed(&account_key(&account))
        .ok_or_else(|| ApiError::new(StatusCode::TOO_MANY_REQUESTS, "too-many-feeds"))?;
    let keep_alive = hub.config().feed_keep_alive;
    let feed = AccountFeed {
        _permit: permit,
        subscription: hub.wakeups.subscribe(&account_key(&account)),
        closing: hub.closing.subscribe(),
        hub,
        account,
        seen: BTreeMap::new(),
        counter: None,
        pending: VecDeque::new(),
        started: false,
    };
    let stream = futures_util::stream::unfold(feed, AccountFeed::next);
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(keep_alive)))
}
