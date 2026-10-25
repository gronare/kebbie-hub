use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use kebbie_hub_api::{
    AccountReveal, CONNECT_DENIED, CONNECT_EXPIRED, CONNECT_PENDING, CONNECT_SLOW_DOWN,
    CONNECT_UNKNOWN, ConnectApproval, ConnectApproved, ConnectDenial, ConnectPoll,
    ConnectRequestView, ConnectStart, ConnectStarted, ConnectState, Connected, PairingState,
    PairingView, code_from_random, normalize_code,
};
use kebbie_hub_protocol::{Id, Prefix};
use serde::Deserialize;

use crate::auth::{
    AccountAuth, Anonymous, DEVICE_CODE_PREFIX, FreshAccount, MACHINE_TOKEN_PREFIX, SameOrigin,
    digest, new_token, random,
};
use crate::error::ApiError;
use crate::pairing::{Side, Watch, view as pairing_view};
use crate::store::{self, Connect, ConnectPairing, NewConnect, Pairing, Store};
use crate::wake::pairing_key;
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
    if request.commitment.is_some() && hub.store().account_key(&account)?.is_none() {
        return Err(ApiError::conflict("no-key"));
    }
    let machine = Id::new(Prefix::Machine).expect("a fresh machine id is valid");
    let pairing_id = URL_SAFE_NO_PAD.encode(random::<16>());
    let unusable = digest(&new_token("unused_"));
    let pairing = request.commitment.map(|commitment| ConnectPairing {
        id: &pairing_id,
        code_hash: &unusable,
        commitment: commitment.0,
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
        None,
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

fn account_pairing(store: &Store, account: &str, id: &str) -> Result<Pairing, ApiError> {
    store
        .pairing(id)?
        .filter(|pairing| pairing.account.as_deref() == Some(account))
        .ok_or_else(ApiError::not_found)
}

pub async fn pairing(
    State(hub): State<Arc<Hub>>,
    AccountAuth(account): AccountAuth,
    Path(id): Path<String>,
    Query(watch): Query<Watch>,
) -> Result<Json<PairingView>, ApiError> {
    let pairing = hub
        .wait_until(&pairing_key(&id), watch.wait, || {
            let pairing = account_pairing(&hub.store(), &account, &id)?;
            let changed = watch.changed(pairing.state_at(now()));
            Ok((pairing, changed))
        })
        .await?;
    let mut shown = pairing_view(&pairing, Side::Device, now());
    shown.machine_name = hub.store().machine_name(&pairing.machine)?;
    Ok(Json(shown))
}

pub async fn reveal(
    State(hub): State<Arc<Hub>>,
    _: SameOrigin,
    AccountAuth(account): AccountAuth,
    Path(id): Path<String>,
    Json(reveal): Json<AccountReveal>,
) -> Result<Json<PairingView>, ApiError> {
    let store = hub.store();
    let mut pairing = account_pairing(&store, &account, &id)?;
    let at = now();
    if pairing.state_at(at) != PairingState::Replied {
        return Err(ApiError::conflict("wrong-state"));
    }
    if store.account_key(&account)?.as_ref() != Some(&reveal.key) {
        return Err(ApiError::conflict("keys-differ"));
    }
    pairing.device_keys = Some(reveal.key.keys);
    pairing.account_key_version = Some(reveal.key.version);
    pairing.device_nonce = Some(reveal.nonce.0);
    pairing.state = PairingState::Revealed;
    store.save_pairing(&pairing)?;
    store.log(at, "pairing.revealed", Some(&pairing.machine), None, Some(&account))?;
    drop(store);
    hub.wake_pairing(&pairing);
    Ok(Json(pairing_view(&pairing, Side::Device, at)))
}
