use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use kebbie_hub_api::{
    CommitRequest, Committed, DeviceName, PairingFailure, PairingState, PairingView,
    RevealRequest, normalize_code,
};
use kebbie_hub_protocol::{Id, Prefix};

use crate::auth::{Anonymous, DEVICE_TOKEN_PREFIX, DeviceAuth, device_for, digest, new_token};
use crate::error::ApiError;
use crate::pairing::{Side, Watch, view};
use crate::store::{Pairing, Store};
use crate::wake::pairing_key;
use crate::{Hub, clean_name, now};

pub async fn commit(
    State(hub): State<Arc<Hub>>,
    client: Anonymous,
    Json(request): Json<CommitRequest>,
) -> Result<Json<Committed>, ApiError> {
    let known = match client.bearer.as_deref() {
        Some(token) => {
            let device = device_for(&hub, Some(token))?
                .ok_or_else(|| hub.refuse_anonymous(&client.address))?;
            hub.limit(&format!("token:{device}"))?;
            Some(device)
        }
        None if hub.config().anonymous_devices => {
            hub.limit_anonymous(&client.address)?;
            None
        }
        None => return Err(hub.refuse_anonymous(&client.address)),
    };
    let code = normalize_code(&request.code).ok_or_else(ApiError::unauthorized)?;
    let at = now();
    let store = hub.store();
    let mut pairing = store
        .pairing_by_code(&digest(&code))?
        .filter(|pairing| !pairing.state_at(at).is_final())
        .ok_or_else(ApiError::unauthorized)?;
    if let (Some(device), Some(owner)) = (&known, store.machine_owner(&pairing.machine)?)
        && store
            .device_account(device)?
            .is_some_and(|account| account != owner)
    {
        return Err(ApiError::forbidden("not-your-machine"));
    }
    if pairing.state_at(at) != PairingState::Open {
        let retried = known.is_some()
            && pairing.device == known
            && pairing.commitment == Some(request.commitment.0);
        if let (true, Some(device)) = (retried, known) {
            return Ok(Json(Committed {
                pairing: pairing.id,
                device,
                token: None,
                expires_at: pairing.expires_at,
            }));
        }
        pairing.state = PairingState::Failed;
        pairing.reason = Some(PairingFailure::SecondCommitment);
        store.save_pairing(&pairing)?;
        store.log(
            at,
            "pairing.failed",
            Some(&pairing.machine),
            pairing.device.as_ref(),
            Some(PairingFailure::SecondCommitment.name()),
        )?;
        drop(store);
        hub.wake_pairing(&pairing);
        return Err(ApiError::conflict("second-commitment"));
    }
    let (device, token) = match known {
        Some(device) => (device, None),
        None => {
            let device = Id::new(Prefix::Device).expect("a fresh device id is valid");
            let token = new_token(DEVICE_TOKEN_PREFIX);
            store.create_device(&device, &digest(&token), None, None, at)?;
            (device, Some(token))
        }
    };
    pairing.device = Some(device.clone());
    pairing.commitment = Some(request.commitment.0);
    pairing.state = PairingState::Committed;
    store.save_pairing(&pairing)?;
    store.log(
        at,
        "pairing.committed",
        Some(&pairing.machine),
        Some(&device),
        None,
    )?;
    drop(store);
    hub.wake_pairing(&pairing);
    Ok(Json(Committed {
        pairing: pairing.id,
        device,
        token,
        expires_at: pairing.expires_at,
    }))
}

fn own_pairing(store: &Store, device: &Id, id: &str) -> Result<Pairing, ApiError> {
    store
        .pairing(id)?
        .filter(|pairing| pairing.device.as_ref() == Some(device))
        .ok_or_else(ApiError::not_found)
}

pub async fn pairing(
    State(hub): State<Arc<Hub>>,
    DeviceAuth(device): DeviceAuth,
    Path(id): Path<String>,
    Query(watch): Query<Watch>,
) -> Result<Json<PairingView>, ApiError> {
    let pairing = hub
        .wait_until(&pairing_key(&id), watch.wait, || {
            let pairing = own_pairing(&hub.store(), &device, &id)?;
            let changed = watch.changed(pairing.state_at(now()));
            Ok((pairing, changed))
        })
        .await?;
    let mut shown = view(&pairing, Side::Device, now());
    shown.machine_name = hub.store().machine_name(&pairing.machine)?;
    Ok(Json(shown))
}

pub async fn reveal(
    State(hub): State<Arc<Hub>>,
    DeviceAuth(device): DeviceAuth,
    Path(id): Path<String>,
    Json(reveal): Json<RevealRequest>,
) -> Result<Json<PairingView>, ApiError> {
    let store = hub.store();
    let mut pairing = own_pairing(&store, &device, &id)?;
    let at = now();
    if pairing.state_at(at) != PairingState::Replied {
        return Err(ApiError::conflict("wrong-state"));
    }
    if store
        .device_keys(&device)?
        .is_some_and(|admitted| admitted != reveal.keys)
    {
        return Err(ApiError::conflict("keys-differ"));
    }
    pairing.device_keys = Some(reveal.keys);
    pairing.device_nonce = Some(reveal.nonce.0);
    pairing.state = PairingState::Revealed;
    store.save_pairing(&pairing)?;
    store.log(
        at,
        "pairing.revealed",
        Some(&pairing.machine),
        Some(&device),
        None,
    )?;
    drop(store);
    hub.wake_pairing(&pairing);
    Ok(Json(view(&pairing, Side::Device, at)))
}

pub async fn rename(
    State(hub): State<Arc<Hub>>,
    DeviceAuth(device): DeviceAuth,
    Json(request): Json<DeviceName>,
) -> Result<Json<DeviceName>, ApiError> {
    let name = clean_name(&request.name).ok_or(ApiError::bad_request("bad-name"))?;
    let store = hub.store();
    store.rename_device(&device, name)?;
    store.log(now(), "device.renamed", None, Some(&device), None)?;
    Ok(Json(DeviceName {
        name: name.to_string(),
    }))
}
