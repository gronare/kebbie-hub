use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use kebbie_hub_api::{
    AdmittedDevices, BoardStored, BoardUpload, Inbox, JoinRequest, Joined, MachineName, PairingFailure,
    PairingOpened, PairingRefusal, PairingReply, PairingState, PairingView, QueuedMessage,
    code_from_random, normalize_code,
};
use kebbie_hub_protocol::{Envelope, Id, Kind, Prefix, verify};
use serde::Deserialize;

use crate::auth::{Anonymous, MACHINE_TOKEN_PREFIX, MachineAuth, digest, new_token, random};
use crate::error::ApiError;
use crate::pairing::{Side, Watch, view};
use crate::store::{BoardWrite, Pairing, Store};
use crate::wake::{machine_key, pairing_key};
use crate::{Hub, MAX_STORED_INTEGER, clean_name, now};

pub async fn join(
    State(hub): State<Arc<Hub>>,
    client: Anonymous,
    Json(request): Json<JoinRequest>,
) -> Result<Json<Joined>, ApiError> {
    hub.limit_anonymous(&client.address)?;
    let name = clean_name(&request.name).ok_or(ApiError::bad_request("bad-name"))?;
    let code = normalize_code(&request.code).ok_or_else(ApiError::unauthorized)?;
    let machine = Id::new(Prefix::Machine).expect("a fresh machine id is valid");
    let token = new_token(MACHINE_TOKEN_PREFIX);
    let at = now();
    let mut store = hub.store();
    if !store.join(
        &digest(&code),
        at,
        &machine,
        name,
        &request.keys,
        &digest(&token),
    )? {
        return Err(ApiError::unauthorized());
    }
    store.log(at, "machine.joined", Some(&machine), None, None)?;
    Ok(Json(Joined { machine, token }))
}

pub async fn rename(
    State(hub): State<Arc<Hub>>,
    MachineAuth(machine): MachineAuth,
    Json(request): Json<MachineName>,
) -> Result<Json<MachineName>, ApiError> {
    let name = clean_name(&request.name).ok_or(ApiError::bad_request("bad-name"))?;
    let at = now();
    let store = hub.store();
    let before = store
        .machine_name(&machine)?
        .ok_or_else(ApiError::unauthorized)?;
    if before != name {
        store.rename_machine(&machine, name)?;
        store.log(at, "machine.renamed", Some(&machine), None, None)?;
    }
    Ok(Json(MachineName {
        name: name.to_string(),
    }))
}

pub async fn put_board(
    State(hub): State<Arc<Hub>>,
    MachineAuth(machine): MachineAuth,
    Json(upload): Json<BoardUpload>,
) -> Result<Json<BoardStored>, ApiError> {
    hub.require_machine_plan(&machine)?;
    if upload.rev > MAX_STORED_INTEGER {
        return Err(ApiError::bad_request("rev-out-of-range"));
    }
    let envelope = &upload.envelope;
    if envelope.header.kind != Kind::Board {
        return Err(ApiError::bad_request("not-a-board"));
    }
    if envelope.header.machine != machine {
        return Err(ApiError::bad_request("wrong-machine"));
    }
    let at = now();
    let mut store = hub.store();
    let keys = store
        .machine_keys(&machine)?
        .ok_or_else(ApiError::unauthorized)?;
    verify(envelope, &machine, &keys, at).map_err(|error| ApiError::bad_request(error.key()))?;
    let text = envelope.to_json();
    match store.store_board(&machine, upload.rev, &envelope.header, &text, at)? {
        BoardWrite::Stale => return Err(ApiError::conflict("stale-revision")),
        BoardWrite::Replayed => return Err(ApiError::conflict("replayed-envelope")),
        BoardWrite::Stored => {}
    }
    store.log(
        at,
        "board.stored",
        Some(&machine),
        None,
        Some(&format!("rev={} bytes={}", upload.rev, text.len())),
    )?;
    let readers = store.admitted_devices(&machine)?;
    drop(store);
    hub.wake_devices(&readers);
    Ok(Json(BoardStored { rev: upload.rev }))
}

#[derive(Deserialize)]
pub struct InboxQuery {
    #[serde(default)]
    after: u64,
    #[serde(default)]
    wait: u64,
}

pub async fn inbox(
    State(hub): State<Arc<Hub>>,
    MachineAuth(machine): MachineAuth,
    Query(query): Query<InboxQuery>,
) -> Result<Json<Inbox>, ApiError> {
    hub.require_machine_plan(&machine)?;
    if query.after > MAX_STORED_INTEGER {
        return Err(ApiError::bad_request("after-out-of-range"));
    }
    hub.store().acknowledge(&machine, query.after)?;
    let queued = hub
        .wait_until(&machine_key(&machine), query.wait, || {
            let queued = hub.store().queued_after(&machine, query.after)?;
            let ready = !queued.is_empty();
            Ok((queued, ready))
        })
        .await?;
    let messages = queued
        .into_iter()
        .map(|(seq, text)| {
            Envelope::from_json(&text)
                .map(|envelope| QueuedMessage { seq, envelope })
                .map_err(|_| ApiError::internal())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let seq = messages.last().map_or(query.after, |message| message.seq);
    let store = hub.store();
    store.mark_delivered(&machine, query.after, seq, now())?;
    let board_rev = store.board_rev(&machine)?;
    drop(store);
    Ok(Json(Inbox {
        seq,
        messages,
        board_rev,
    }))
}

impl Hub {
    pub(crate) fn open_pairing_for(&self, machine: &Id) -> Result<PairingOpened, ApiError> {
        let id = URL_SAFE_NO_PAD.encode(random::<16>());
        let code = code_from_random(random());
        let at = now();
        let expires_at = at + self.config.pairing_ttl;
        let store = self.store();
        store.open_pairing(&id, machine, &digest(&code), at, expires_at)?;
        store.log(at, "pairing.opened", Some(machine), None, None)?;
        Ok(PairingOpened {
            pairing: id,
            code,
            expires_at,
        })
    }
}

pub async fn open_pairing(
    State(hub): State<Arc<Hub>>,
    MachineAuth(machine): MachineAuth,
) -> Result<Json<PairingOpened>, ApiError> {
    hub.require_machine_plan(&machine)?;
    Ok(Json(hub.open_pairing_for(&machine)?))
}

fn own_pairing(store: &Store, machine: &Id, id: &str) -> Result<Pairing, ApiError> {
    store
        .pairing(id)?
        .filter(|pairing| &pairing.machine == machine)
        .ok_or_else(ApiError::not_found)
}

pub async fn pairing(
    State(hub): State<Arc<Hub>>,
    MachineAuth(machine): MachineAuth,
    Path(id): Path<String>,
    Query(watch): Query<Watch>,
) -> Result<Json<PairingView>, ApiError> {
    let pairing = hub
        .wait_until(&pairing_key(&id), watch.wait, || {
            let pairing = own_pairing(&hub.store(), &machine, &id)?;
            let changed = watch.changed(pairing.state_at(now()));
            Ok((pairing, changed))
        })
        .await?;
    Ok(Json(view(&pairing, Side::Machine, now())))
}

fn advance(
    hub: &Hub,
    machine: &Id,
    id: &str,
    from: PairingState,
    step: impl FnOnce(&Store, &mut Pairing) -> Result<(), ApiError>,
) -> Result<Json<PairingView>, ApiError> {
    let store = hub.store();
    let mut pairing = own_pairing(&store, machine, id)?;
    let at = now();
    if pairing.state_at(at) != from {
        return Err(ApiError::conflict("wrong-state"));
    }
    step(&store, &mut pairing)?;
    store.save_pairing(&pairing)?;
    drop(store);
    hub.wake_pairing(&pairing);
    Ok(Json(view(&pairing, Side::Machine, at)))
}

pub async fn reply(
    State(hub): State<Arc<Hub>>,
    MachineAuth(machine): MachineAuth,
    Path(id): Path<String>,
    Json(reply): Json<PairingReply>,
) -> Result<Json<PairingView>, ApiError> {
    let registered = hub.store().machine_keys(&machine)?;
    if registered != Some(reply.keys) {
        return Err(ApiError::bad_request("keys-differ"));
    }
    advance(
        &hub,
        &machine,
        &id,
        PairingState::Committed,
        |_, pairing| {
            pairing.machine_keys = Some(reply.keys);
            pairing.machine_nonce = Some(reply.nonce.0);
            pairing.state = PairingState::Replied;
            Ok(())
        },
    )
}

pub async fn admit(
    State(hub): State<Arc<Hub>>,
    MachineAuth(machine): MachineAuth,
    Path(id): Path<String>,
) -> Result<Json<PairingView>, ApiError> {
    advance(
        &hub,
        &machine,
        &id,
        PairingState::Revealed,
        |store, pairing| {
            let device = pairing
                .device
                .clone()
                .ok_or_else(|| ApiError::conflict("wrong-state"))?;
            let keys = pairing
                .device_keys
                .ok_or_else(|| ApiError::conflict("wrong-state"))?;
            let at = now();
            store.fix_device_keys(&device, &keys)?;
            store.admit(&machine, &device, at)?;
            store.log(at, "pairing.admitted", Some(&machine), Some(&device), None)?;
            pairing.state = PairingState::Admitted;
            Ok(())
        },
    )
}

pub async fn refuse(
    State(hub): State<Arc<Hub>>,
    MachineAuth(machine): MachineAuth,
    Path(id): Path<String>,
    Json(refusal): Json<PairingRefusal>,
) -> Result<Json<PairingView>, ApiError> {
    if refusal.reason == PairingFailure::SecondCommitment {
        return Err(ApiError::bad_request("bad-reason"));
    }
    let store = hub.store();
    let mut pairing = own_pairing(&store, &machine, &id)?;
    let at = now();
    if pairing.state_at(at).is_final() {
        return Err(ApiError::conflict("wrong-state"));
    }
    pairing.state = PairingState::Refused;
    pairing.reason = Some(refusal.reason);
    store.save_pairing(&pairing)?;
    store.log(
        at,
        "pairing.refused",
        Some(&machine),
        pairing.device.as_ref(),
        Some(refusal.reason.name()),
    )?;
    drop(store);
    hub.wake_pairing(&pairing);
    Ok(Json(view(&pairing, Side::Machine, at)))
}

pub async fn devices(
    State(hub): State<Arc<Hub>>,
    MachineAuth(machine): MachineAuth,
) -> Result<Json<AdmittedDevices>, ApiError> {
    let devices = hub.store().admitted_device_views(&machine)?;
    Ok(Json(AdmittedDevices { devices }))
}

pub async fn remove_device(
    State(hub): State<Arc<Hub>>,
    MachineAuth(machine): MachineAuth,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let device = Id::parse(&id).map_err(|_| ApiError::not_found())?;
    let at = now();
    {
        let store = hub.store();
        if !store.remove_admission(&machine, &device)? {
            return Err(ApiError::not_found());
        }
        store.log(at, "admission.removed", Some(&machine), Some(&device), None)?;
    }
    hub.wake_device(&device);
    Ok(StatusCode::NO_CONTENT)
}
