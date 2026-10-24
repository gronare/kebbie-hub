use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use kelpie_hub_api::{
    AddView, CONNECT_DENIED, CONNECT_EXPIRED, CONNECT_PENDING, CONNECT_SLOW_DOWN, CONNECT_UNKNOWN,
    ConnectApproval, ConnectApproved, ConnectDenial, ConnectPoll, ConnectRequestView, ConnectStart,
    ConnectStarted, ConnectState, Connected, DeviceAdd, PairingState, code_from_random,
    normalize_code,
};
use kelpie_hub_protocol::{Id, Prefix};
use serde::Deserialize;

use crate::auth::{
    AccountAuth, Anonymous, DEVICE_CODE_PREFIX, FreshAccount, MACHINE_TOKEN_PREFIX, MachineAuth,
    SameOrigin, digest, new_token, random,
};
use crate::error::ApiError;
use crate::store::{self, Connect, ConnectPairing, NewConnect};
use crate::{Hub, clean_name, now};

const SLOW_DOWN_STEP: u64 = 5;

fn grant(code: &'static str) -> ApiError {
    ApiError::bad_request(code)
}

pub async fn start(
    State(hub): State<Arc<Hub>>,
    client: Anonymous,
    Json(request): Json<ConnectStart>,
) -> Result<Json<ConnectStarted>, ApiError> {
    if !hub
        .connect_limiter
        .allow(&format!("address:{}", client.address))
    {
        return Err(ApiError::rate_limited());
    }
    let name = clean_name(&request.name).ok_or(ApiError::bad_request("bad-name"))?;
    let device_code = new_token(DEVICE_CODE_PREFIX);
    let user_code = code_from_random(random());
    let at = now();
    let ttl = hub.config().connect_ttl;
    let interval = hub.config().connect_interval;
    let store = hub.store();
    store.create_connect(&NewConnect {
        device_code_hash: &digest(&device_code),
        user_code_hash: &digest(&user_code),
        name,
        keys: &request.keys,
        interval,
        now: at,
        expires_at: at + ttl,
    })?;
    store.log(at, "connect.started", None, None, None)?;
    let verification_uri = format!("{}/connect", hub.origin());
    Ok(Json(ConnectStarted {
        verification_uri_complete: format!("{verification_uri}?code={user_code}"),
        verification_uri,
        device_code,
        user_code,
        interval,
        expires_in: ttl,
    }))
}

pub async fn token(
    State(hub): State<Arc<Hub>>,
    client: Anonymous,
    Json(request): Json<ConnectPoll>,
) -> Result<Json<Connected>, ApiError> {
    hub.limit_anonymous(&client.address)?;
    let hash = digest(&request.device_code);
    let at = now();
    let mut store = hub.store();
    let Some(connect) = request
        .device_code
        .starts_with(DEVICE_CODE_PREFIX)
        .then(|| store.connect_by_device_code(&hash))
        .transpose()?
        .flatten()
    else {
        return Err(hub.refuse_grant(&client.address));
    };
    if at >= connect.expires_at {
        return Err(grant(CONNECT_EXPIRED));
    }
    if connect
        .polled_at
        .is_some_and(|polled| at.saturating_sub(polled) < connect.interval)
    {
        store.polled_connect(&hash, at, connect.interval + SLOW_DOWN_STEP)?;
        return Err(grant(CONNECT_SLOW_DOWN));
    }
    match connect.state {
        ConnectState::Pending => {
            store.polled_connect(&hash, at, connect.interval)?;
            Err(grant(CONNECT_PENDING))
        }
        ConnectState::Denied => {
            store.end_connect(&hash)?;
            Err(grant(CONNECT_DENIED))
        }
        ConnectState::Approved => {
            let token = new_token(MACHINE_TOKEN_PREFIX);
            let (machine, pairing) = store
                .redeem_connect(&hash, &digest(&token), at)?
                .ok_or_else(|| grant(CONNECT_UNKNOWN))?;
            store.log(
                at,
                "machine.joined",
                Some(&machine),
                None,
                Some("by=connect"),
            )?;
            Ok(Json(Connected {
                machine,
                token,
                pairing,
            }))
        }
    }
}

impl Hub {
    fn refuse_grant(&self, address: &str) -> ApiError {
        match self.limit_anonymous(address) {
            Ok(()) => grant(CONNECT_UNKNOWN),
            Err(limited) => limited,
        }
    }

    fn limit_code(&self, account: &str) -> Result<(), ApiError> {
        if self.code_limiter.allow(&format!("account:{account}")) {
            Ok(())
        } else {
            Err(ApiError::rate_limited())
        }
    }
}

#[derive(Deserialize)]
pub struct CodeQuery {
    code: String,
}

fn pending(store: &store::Store, typed: &str, at: u64) -> Result<(String, Connect), ApiError> {
    let code = normalize_code(typed).ok_or_else(ApiError::not_found)?;
    let connect = store
        .connect_by_user_code(&digest(&code))?
        .filter(|connect| connect.state == ConnectState::Pending && at < connect.expires_at)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "unknown-code"))?;
    Ok((code, connect))
}

pub async fn view(
    State(hub): State<Arc<Hub>>,
    AccountAuth(account): AccountAuth,
    Query(query): Query<CodeQuery>,
) -> Result<Json<ConnectRequestView>, ApiError> {
    hub.limit_code(&account)?;
    let (code, connect) = pending(&hub.store(), &query.code, now())?;
    Ok(Json(ConnectRequestView {
        code,
        name: connect.name,
        state: connect.state,
        expires_at: connect.expires_at,
    }))
}

pub async fn approve(
    State(hub): State<Arc<Hub>>,
    _: SameOrigin,
    FreshAccount(account): FreshAccount,
    Json(request): Json<ConnectApproval>,
) -> Result<Json<ConnectApproved>, ApiError> {
    hub.limit_code(&account)?;
    hub.require_account_plan(&account)?;
    let at = now();
    let browser = match (&request.device, &request.commitment) {
        (Some(device), Some(commitment)) => {
            if hub.store().device_account(device)?.as_deref() != Some(account.as_str()) {
                return Err(ApiError::new(StatusCode::NOT_FOUND, "unknown-device"));
            }
            Some((device.clone(), commitment.0))
        }
        (None, None) => None,
        _ => return Err(ApiError::bad_request("commitment-needs-device")),
    };
    let machine = Id::new(Prefix::Machine).expect("a fresh machine id is valid");
    let pairing_id = URL_SAFE_NO_PAD.encode(random::<16>());
    let unusable = digest(&new_token("unused_"));
    let pairing = browser.as_ref().map(|(device, commitment)| ConnectPairing {
        id: &pairing_id,
        code_hash: &unusable,
        device,
        commitment: *commitment,
        expires_at: at + hub.config().pairing_ttl,
    });
    let mut store = hub.store();
    let (code, _) = pending(&store, &request.code, at)?;
    let placeholder = random::<32>();
    let approved = store
        .approve_connect(&store::ConnectApproval {
            user_code_hash: &digest(&code),
            account: &account,
            machine: &machine,
            placeholder_token_hash: &placeholder,
            pairing: pairing.as_ref(),
            now: at,
        })?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "unknown-code"))?;
    store.log(
        at,
        "connect.approved",
        Some(&machine),
        browser.as_ref().map(|(device, _)| device),
        Some(&account),
    )?;
    Ok(Json(ConnectApproved {
        machine,
        name: approved.name,
        pairing: approved.pairing,
    }))
}

pub async fn deny(
    State(hub): State<Arc<Hub>>,
    _: SameOrigin,
    AccountAuth(account): AccountAuth,
    Json(request): Json<ConnectDenial>,
) -> Result<StatusCode, ApiError> {
    hub.limit_code(&account)?;
    let at = now();
    let store = hub.store();
    let (code, _) = pending(&store, &request.code, at)?;
    store.deny_connect(&digest(&code), at)?;
    store.log(at, "connect.denied", None, None, Some(&account))?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn add_device(
    State(hub): State<Arc<Hub>>,
    MachineAuth(machine): MachineAuth,
) -> Result<Json<DeviceAdd>, ApiError> {
    hub.require_machine_plan(&machine)?;
    let opened = hub.open_pairing_for(&machine)?;
    Ok(Json(DeviceAdd {
        url: format!("{}/add?code={}", hub.origin(), opened.code),
        pairing: opened.pairing,
        code: opened.code,
        expires_at: opened.expires_at,
    }))
}

pub async fn add_view(
    State(hub): State<Arc<Hub>>,
    AccountAuth(account): AccountAuth,
    Query(query): Query<CodeQuery>,
) -> Result<Json<AddView>, ApiError> {
    hub.limit_code(&account)?;
    let code = normalize_code(&query.code).ok_or_else(ApiError::not_found)?;
    let at = now();
    let store = hub.store();
    let pairing = store
        .pairing_by_code(&digest(&code))?
        .filter(|pairing| pairing.state_at(at) == PairingState::Open)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "unknown-code"))?;
    if store
        .machine_owner(&pairing.machine)?
        .is_some_and(|owner| owner != account)
    {
        return Err(ApiError::forbidden("not-your-machine"));
    }
    let name = store
        .machine_name(&pairing.machine)?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(AddView {
        machine: pairing.machine,
        name,
        expires_at: pairing.expires_at,
    }))
}
