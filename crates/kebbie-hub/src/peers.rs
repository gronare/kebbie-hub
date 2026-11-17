use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use kebbie_hub_api::{
    AccountMachine, AccountMachines, AnnouncedAddresses, Bytes64, MessageQueued, MessageUpload, NO_ACCOUNT, NO_JOB, NOT_A_WORKER,
    SignedMachineList, WorkerOs, is_worker_os,
};
use kebbie_hub_protocol::{Envelope, Id, Kind, OpenError, Prefix, verify};

use crate::auth::MachineAuth;
use crate::error::ApiError;
use crate::store::Store;
use crate::wake::machine_key;
use crate::{Hub, now};

const MAX_MESSAGE_CT: usize = 48 * 1024;
// A worker reports on a job for this long after the job could last be delivered, which covers a long build.
const REPORT_WINDOW: u64 = 86_400;

fn account_of(store: &Store, machine: &Id) -> Result<String, ApiError> {
    store.machine_owner(machine)?.ok_or(ApiError::conflict(NO_ACCOUNT))
}

pub async fn set_worker(
    State(hub): State<Arc<Hub>>,
    MachineAuth(machine): MachineAuth,
    Json(request): Json<WorkerOs>,
) -> Result<Json<WorkerOs>, ApiError> {
    if !is_worker_os(&request.os) {
        return Err(ApiError::bad_request("bad-os"));
    }
    let store = hub.store();
    account_of(&store, &machine)?;
    store.set_worker_os(&machine, Some(&request.os))?;
    store.log(now(), "worker.set", Some(&machine), Some(&request.os))?;
    Ok(Json(request))
}

pub async fn clear_worker(
    State(hub): State<Arc<Hub>>,
    MachineAuth(machine): MachineAuth,
) -> Result<StatusCode, ApiError> {
    let store = hub.store();
    account_of(&store, &machine)?;
    store.set_worker_os(&machine, None)?;
    store.log(now(), "worker.cleared", Some(&machine), None)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn machines(
    State(hub): State<Arc<Hub>>,
    MachineAuth(machine): MachineAuth,
) -> Result<Json<AccountMachines>, ApiError> {
    let store = hub.store();
    let account = account_of(&store, &machine)?;
    let list = store.machine_list(&account)?.map(|(list, signature)| SignedMachineList {
        list,
        signature: Bytes64(signature),
    });
    let machines = store
        .account_machines(&account)?
        .into_iter()
        .map(|row| AccountMachine {
            machine: row.machine,
            name: row.name,
            worker_os: row.worker_os,
            last_seen: row.last_seen,
        })
        .collect();
    Ok(Json(AccountMachines { list, machines }))
}

// A machine message the hub may pass on: from this machine, sealed to exactly another machine of its account, this machine and the account, and signed by this machine.
fn checked(store: &Store, machine: &Id, envelope: &Envelope, at: u64) -> Result<String, ApiError> {
    let header = &envelope.header;
    header
        .check_shape()
        .map_err(|shape| ApiError::bad_request(OpenError::from(shape).key()))?;
    if !header.kind.is_machine_message() {
        return Err(ApiError::bad_request("not-a-machine-message"));
    }
    if &header.from != machine {
        return Err(ApiError::bad_request(OpenError::WrongSender.key()));
    }
    let target = &header.machine;
    let account = account_of(store, machine)?;
    if store.machine_owner(target)?.as_deref() != Some(account.as_str()) {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "unknown-machine"));
    }
    let addressed = envelope.keys.len() == 3
        && envelope.keys.contains_key(target)
        && envelope.keys.contains_key(machine)
        && envelope.keys.keys().any(|recipient| recipient.prefix() == Prefix::Account && recipient.as_str() == account);
    if !addressed {
        return Err(ApiError::bad_request("bad-recipients"));
    }
    if envelope.ct.len() > MAX_MESSAGE_CT {
        return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "too-large"));
    }
    let keys = store
        .machine_keys(machine)?
        .ok_or_else(ApiError::unauthorized)?;
    verify(envelope, machine, &keys, at).map_err(|error| ApiError::bad_request(error.key()))?;
    Ok(account)
}

pub async fn post_message(
    State(hub): State<Arc<Hub>>,
    MachineAuth(machine): MachineAuth,
    Json(upload): Json<MessageUpload>,
) -> Result<(StatusCode, Json<MessageQueued>), ApiError> {
    let envelope = &upload.envelope;
    let header = &envelope.header;
    if header.kind == Kind::Addresses {
        return Err(ApiError::bad_request("not-a-machine-message"));
    }
    let target = &header.machine;
    let at = now();
    let mut store = hub.store();
    checked(&store, &machine, envelope, at)?;
    if header.kind == Kind::Job && store.worker_os(target)?.is_none() {
        return Err(ApiError::conflict(NOT_A_WORKER));
    }
    if header.kind == Kind::JobReport && !store.reports_allowed(&machine, target, at)? {
        return Err(ApiError::conflict(NO_JOB));
    }
    if header.kind == Kind::Job {
        hub.check_command_gate(&store, target, header.kind)?;
    }
    if store.queued_count(target)? >= hub.config().max_queued {
        return Err(ApiError::new(StatusCode::TOO_MANY_REQUESTS, "queue-full"));
    }
    let kept = envelope
        .for_recipient(target)
        .ok_or_else(ApiError::internal)?
        .to_json();
    let seq = store.enqueue(target, &kept, at, header.exp)?;
    if header.kind == Kind::Job {
        store.allow_reports(target, &machine, header.exp + REPORT_WINDOW)?;
    }
    store.log(
        at,
        "message.queued",
        Some(target),
        Some(&format!("kind={} from={machine} bytes={}", header.kind.name(), kept.len())),
    )?;
    drop(store);
    hub.wakeups.wake(&machine_key(target));
    Ok((
        StatusCode::ACCEPTED,
        Json(MessageQueued {
            message: header.id.clone(),
            seq,
        }),
    ))
}

// A machine's addresses never enter an inbox, where a kebbie that predates them would stop on a kind it cannot read; the hub keeps the latest one from each machine to each other machine instead, read by those that ask.
pub async fn put_addresses(
    State(hub): State<Arc<Hub>>,
    MachineAuth(machine): MachineAuth,
    Json(upload): Json<MessageUpload>,
) -> Result<StatusCode, ApiError> {
    let envelope = &upload.envelope;
    let header = &envelope.header;
    if header.kind != Kind::Addresses {
        return Err(ApiError::bad_request("not-addresses"));
    }
    let at = now();
    let store = hub.store();
    checked(&store, &machine, envelope, at)?;
    let kept = envelope
        .for_recipient(&header.machine)
        .ok_or_else(ApiError::internal)?
        .to_json();
    store.put_addresses(&header.machine, &machine, &kept, header.exp)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn addresses(
    State(hub): State<Arc<Hub>>,
    MachineAuth(machine): MachineAuth,
) -> Result<Json<AnnouncedAddresses>, ApiError> {
    let store = hub.store();
    account_of(&store, &machine)?;
    let envelopes = store
        .addresses_for(&machine, now())?
        .iter()
        .map(|text| Envelope::from_json(text).map_err(|_| ApiError::internal()))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(AnnouncedAddresses { envelopes }))
}
