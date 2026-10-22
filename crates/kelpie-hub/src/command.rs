use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use kelpie_hub_api::{
    CommandQueued, CommandUpload, ResultEntry, ResultStored, ResultUpload, Results,
};
use kelpie_hub_protocol::{Envelope, Kind, OpenError, Prefix, verify};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::auth::{DeviceAuth, MachineAuth};
use crate::error::ApiError;
use crate::store::{CommandWrite, NewCommand, NewResult, ResultWrite};
use crate::wake::{device_key, machine_key};
use crate::{Hub, MAX_STORED_INTEGER, now};

const MAX_TEXT_COMMAND_CT: usize = 36 * 1024;
const MAX_SHORT_COMMAND_CT: usize = 4 * 1024;

fn max_ciphertext(kind: Kind) -> usize {
    match kind {
        Kind::Answer | Kind::Start => MAX_TEXT_COMMAND_CT,
        _ => MAX_SHORT_COMMAND_CT,
    }
}

fn refused(error: OpenError) -> ApiError {
    ApiError::bad_request(error.key())
}

pub async fn post_command(
    State(hub): State<Arc<Hub>>,
    DeviceAuth(device): DeviceAuth,
    Json(upload): Json<CommandUpload>,
) -> Result<(StatusCode, Json<CommandQueued>), ApiError> {
    let envelope = &upload.envelope;
    let header = &envelope.header;
    header
        .check_shape()
        .map_err(|shape| refused(OpenError::from(shape)))?;
    if !header.kind.is_command() {
        return Err(ApiError::bad_request("not-a-command"));
    }
    if header.from != device {
        return Err(refused(OpenError::WrongSender));
    }
    if envelope.ct.len() > max_ciphertext(header.kind) {
        return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "too-large"));
    }
    let machine = &header.machine;
    let addressed = envelope.keys.contains_key(machine)
        && envelope
            .keys
            .keys()
            .all(|recipient| recipient == machine || recipient == &device);
    if !addressed {
        return Err(ApiError::bad_request("bad-recipients"));
    }
    let text = envelope.to_json();
    let digest = Sha256::digest(text.as_bytes()).to_vec();
    let queued = |seq| {
        (
            StatusCode::ACCEPTED,
            Json(CommandQueued {
                command: header.id.clone(),
                seq,
            }),
        )
    };
    let at = now();
    let mut store = hub.store();
    if !store.is_admitted(machine, &device)? {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "unknown-machine"));
    }
    if let Some((held, seq)) = store.resent_command(&device, &header.id)?
        && held == digest
    {
        return Ok(queued(seq));
    }
    let keys = store.device_keys(&device)?.ok_or_else(ApiError::internal)?;
    verify(envelope, &device, &keys, at).map_err(refused)?;
    hub.limit_command(&device, header.kind)?;
    if store.queued_count(machine)? >= hub.config().max_queued
        || store.undelivered_count(machine, &device)? >= hub.config().max_undelivered_per_device
    {
        return Err(ApiError::new(StatusCode::TOO_MANY_REQUESTS, "queue-full"));
    }
    let written = store.queue_command(&NewCommand {
        device: &device,
        command: &header.id,
        machine,
        kind: header.kind.name(),
        digest: &digest,
        envelope: &text,
        now: at,
        expires_at: header.exp,
    })?;
    let seq = match written {
        CommandWrite::Queued(seq) => seq,
        CommandWrite::Resent(seq) => return Ok(queued(seq)),
        CommandWrite::IdTaken => return Err(ApiError::conflict("command-id-taken")),
    };
    store.log(
        at,
        "command.queued",
        Some(machine),
        Some(&device),
        Some(&format!("kind={} bytes={}", header.kind.name(), text.len())),
    )?;
    drop(store);
    hub.wakeups.wake(&machine_key(machine));
    Ok(queued(seq))
}

pub async fn post_result(
    State(hub): State<Arc<Hub>>,
    MachineAuth(machine): MachineAuth,
    Json(upload): Json<ResultUpload>,
) -> Result<Json<ResultStored>, ApiError> {
    let envelope = &upload.envelope;
    let header = &envelope.header;
    if header.kind != Kind::Result {
        return Err(ApiError::bad_request("not-a-result"));
    }
    if header.machine != machine {
        return Err(ApiError::bad_request("wrong-machine"));
    }
    let mut devices = envelope
        .keys
        .keys()
        .filter(|recipient| **recipient != machine);
    let device = match (devices.next(), devices.next()) {
        (Some(device), None) if device.prefix() == Prefix::Device => device.clone(),
        _ => return Err(ApiError::bad_request("bad-recipients")),
    };
    let at = now();
    let mut store = hub.store();
    let keys = store
        .machine_keys(&machine)?
        .ok_or_else(ApiError::unauthorized)?;
    verify(envelope, &machine, &keys, at).map_err(refused)?;
    let command = store
        .delivered_command(&machine, &header.id)?
        .filter(|command| command.device == device)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "unknown-command"))?;
    if !command.delivered {
        return Err(ApiError::conflict("not-delivered"));
    }
    let text = envelope.to_json();
    let written = store.store_result(&NewResult {
        device: &device,
        machine: &machine,
        command: &header.id,
        digest: &Sha256::digest(text.as_bytes()),
        envelope: &text,
        now: at,
        expires_at: header.exp,
    })?;
    let seq = match written {
        ResultWrite::Stored(seq) => seq,
        ResultWrite::Resent(seq) => {
            return Ok(Json(ResultStored {
                command: header.id.clone(),
                seq,
            }));
        }
        ResultWrite::Differs => return Err(ApiError::conflict("result-differs")),
    };
    store.log(at, "result.stored", Some(&machine), Some(&device), None)?;
    drop(store);
    hub.wakeups.wake(&device_key(&device));
    Ok(Json(ResultStored {
        command: header.id.clone(),
        seq,
    }))
}

#[derive(Deserialize)]
pub struct ResultsQuery {
    #[serde(default)]
    after: u64,
    #[serde(default)]
    wait: u64,
}

pub async fn results(
    State(hub): State<Arc<Hub>>,
    DeviceAuth(device): DeviceAuth,
    Query(query): Query<ResultsQuery>,
) -> Result<Json<Results>, ApiError> {
    if query.after > MAX_STORED_INTEGER {
        return Err(ApiError::bad_request("after-out-of-range"));
    }
    let stored = hub
        .wait_until(&device_key(&device), query.wait, || {
            let stored = hub.store().results_after(&device, query.after, now())?;
            let ready = !stored.is_empty();
            Ok((stored, ready))
        })
        .await?;
    let results = stored
        .into_iter()
        .map(|result| {
            let envelope = Envelope::from_json(&result.envelope)
                .ok()
                .and_then(|envelope| envelope.for_recipient(&device))
                .ok_or_else(ApiError::internal)?;
            Ok(ResultEntry {
                seq: result.seq,
                machine: result.machine,
                command: result.command,
                stored_at: result.stored_at,
                envelope,
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    let cursor = results.last().map_or(query.after, |result| result.seq);
    Ok(Json(Results { cursor, results }))
}
