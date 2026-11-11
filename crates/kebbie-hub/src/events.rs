use std::collections::{BTreeMap, VecDeque};
use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::Stream;
use kebbie_hub_api::{
    BoardChanged, EVENT_BOARD, EVENT_MACHINE_REMOVED, EVENT_MACHINES, EVENT_RESULT,
    MachineRemoved, MachinesChanged, ResultReady,
};
use kebbie_hub_protocol::Id;
use serde::Serialize;
use tokio::sync::watch;

use crate::auth::KeyAccount;
use crate::error::ApiError;
use crate::store::BoardHead;
use crate::wake::{Subscription, account_key};
use crate::{FeedPermit, Hub, MAX_STORED_INTEGER, now};

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

async fn woken(subscription: &mut Subscription, closing: &mut watch::Receiver<bool>) -> bool {
    let closed = async {
        let _ = closing.wait_for(|closing| *closing).await;
    };
    tokio::select! {
        () = subscription.changed() => true,
        () = closed => false,
    }
}

struct AccountFeed {
    _permit: FeedPermit,
    hub: Arc<Hub>,
    account: String,
    subscription: Subscription,
    closing: watch::Receiver<bool>,
    seen: BTreeMap<Id, BoardHead>,
    counter: Option<u64>,
    results_after: u64,
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
            if self.started && !woken(&mut self.subscription, &mut self.closing).await {
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
            let Ok(results) =
                self.hub
                    .store()
                    .result_heads_after(&self.account, self.results_after, now())
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

pub async fn account_events(
    State(hub): State<Arc<Hub>>,
    KeyAccount(account): KeyAccount,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let results_after = match headers.get("last-event-id") {
        None => hub.store().latest_result_seq(&account)?,
        Some(value) => value
            .to_str()
            .ok()
            .and_then(|text| text.trim().parse::<u64>().ok())
            .filter(|seq| *seq <= MAX_STORED_INTEGER)
            .ok_or(ApiError::bad_request("bad-last-event-id"))?,
    };
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
        results_after,
        pending: VecDeque::new(),
        started: false,
    };
    let stream = futures_util::stream::unfold(feed, AccountFeed::next);
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(keep_alive)))
}
