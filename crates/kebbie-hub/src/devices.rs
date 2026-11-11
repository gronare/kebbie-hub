use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use kebbie_hub_api::{Bytes32, DeviceCommand, DeviceCommands, DeviceUpload, DeviceView, Devices};
use kebbie_hub_protocol::{Envelope, Id, MAX_DEVICE_NAME_CHARS, Prefix};
use serde::Deserialize;

use crate::auth::{KeyAccount, SameOrigin};
use crate::error::ApiError;
use crate::store::{DeviceRow, DeviceWrite, NewDevice};
use crate::{Hub, MAX_STORED_INTEGER, now};

const PAGE: u64 = 50;

fn view(row: DeviceRow) -> DeviceView {
    DeviceView {
        device: row.cert.device.clone(),
        name: row.cert.name.clone(),
        kind: row.cert.kind,
        signing: Bytes32(row.cert.signing),
        fingerprint: row.cert.fingerprint(),
        cert: row.cert,
        created_at: row.created_at,
        last_seen_at: row.last_seen_at,
    }
}

pub async fn post_device(
    State(hub): State<Arc<Hub>>,
    _: SameOrigin,
    KeyAccount(account): KeyAccount,
    Json(upload): Json<DeviceUpload>,
) -> Result<(StatusCode, Json<DeviceView>), ApiError> {
    let cert = &upload.cert;
    let named = cert.name.chars().count();
    if named == 0 || named > MAX_DEVICE_NAME_CHARS {
        return Err(ApiError::bad_request("bad-name"));
    }
    if cert.account.as_str() != account {
        return Err(ApiError::bad_request("wrong-account"));
    }
    let mut store = hub.store();
    let key = store
        .account_key(&account)?
        .ok_or(ApiError::conflict("no-key"))?;
    if !key.verifies_device(cert) {
        return Err(ApiError::bad_request("bad-certificate"));
    }
    let text = serde_json::to_string(cert).map_err(|_| ApiError::internal())?;
    let at = now();
    let written = store.register_device(&NewDevice {
        account: &account,
        cert,
        text: &text,
        now: at,
    })?;
    let status = match written {
        DeviceWrite::Added => StatusCode::CREATED,
        DeviceWrite::Renamed => StatusCode::OK,
        DeviceWrite::Taken => return Err(ApiError::conflict("device-taken")),
    };
    let row = store.device(&cert.device)?.ok_or_else(ApiError::internal)?;
    store.log(
        at,
        if status == StatusCode::CREATED {
            "device.added"
        } else {
            "device.renamed"
        },
        None,
        None,
    )?;
    Ok((status, Json(view(row))))
}

pub async fn devices(
    State(hub): State<Arc<Hub>>,
    KeyAccount(account): KeyAccount,
) -> Result<Json<Devices>, ApiError> {
    let rows = hub.store().devices(&account)?;
    Ok(Json(Devices {
        devices: rows.into_iter().map(view).collect(),
    }))
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    before: Option<u64>,
}

pub async fn device_commands(
    State(hub): State<Arc<Hub>>,
    KeyAccount(account): KeyAccount,
    Path(id): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<DeviceCommands>, ApiError> {
    if query
        .before
        .is_some_and(|before| before > MAX_STORED_INTEGER)
    {
        return Err(ApiError::bad_request("before-out-of-range"));
    }
    let unknown = || ApiError::new(StatusCode::NOT_FOUND, "unknown-device");
    let device = Id::parse(&id)
        .ok()
        .filter(|device| device.prefix() == Prefix::Device)
        .ok_or_else(unknown)?;
    let reader = Id::parse(&account).map_err(|_| ApiError::internal())?;
    let rows = {
        let store = hub.store();
        if store
            .device(&device)?
            .is_none_or(|row| row.account != account)
        {
            return Err(unknown());
        }
        store.device_commands(&account, &device, query.before, PAGE + 1)?
    };
    let more = rows.len() as u64 > PAGE;
    let opened = |text: &str| {
        Envelope::from_json(text)
            .ok()
            .and_then(|envelope| envelope.for_recipient(&reader))
            .ok_or_else(ApiError::internal)
    };
    let commands = rows
        .iter()
        .take(PAGE as usize)
        .map(|row| {
            Ok(DeviceCommand {
                command: row.command.clone(),
                machine: row.machine.clone(),
                kind: row.kind.clone(),
                queued_at: row.queued_at,
                delivered_at: row.delivered_at,
                envelope: opened(&row.envelope)?,
                result: row.result.as_deref().map(opened).transpose()?,
                result_at: row.result_at,
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    let cursor = if more {
        rows.get(PAGE as usize - 1).map(|row| row.position)
    } else {
        None
    };
    Ok(Json(DeviceCommands { commands, cursor }))
}
