use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::HeaderMap;
use axum::http::header::{AUTHORIZATION, COOKIE, ORIGIN};
use axum::http::request::Parts;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use kelpie_hub_protocol::Id;
use sha2::{Digest, Sha256};

use crate::error::ApiError;
use crate::limit::address_key;
use crate::{Hub, TrustedProxy};

pub const MACHINE_TOKEN_PREFIX: &str = "khm_";
pub const DEVICE_TOKEN_PREFIX: &str = "khd_";
pub const SESSION_TOKEN_PREFIX: &str = "khs_";
pub const SESSION_COOKIE: &str = "__Host-kelpie_session";

pub fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).expect("the system random source failed");
    bytes
}

pub fn new_token(prefix: &str) -> String {
    format!("{prefix}{}", URL_SAFE_NO_PAD.encode(random::<32>()))
}

pub fn digest(secret: &str) -> Vec<u8> {
    Sha256::digest(secret.as_bytes()).to_vec()
}

pub fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|token| !token.is_empty())
}

pub fn session_cookie(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == SESSION_COOKIE)
        .map(|(_, value)| value.trim())
        .filter(|token| token.starts_with(SESSION_TOKEN_PREFIX))
}

const CLOUDFLARE_CLIENT_HEADER: &str = "cf-connecting-ip";

pub fn client_address(parts: &Parts, hub: &Hub) -> String {
    let forwarded = match hub.config().trusted_proxy {
        Some(TrustedProxy::Cloudflare) => parts
            .headers
            .get(CLOUDFLARE_CLIENT_HEADER)
            .and_then(|value| value.to_str().ok())
            .and_then(|text| text.trim().parse::<IpAddr>().ok()),
        None => None,
    };
    let peer = || {
        parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(address)| address.ip())
    };
    forwarded
        .or_else(peer)
        .map_or_else(|| "unknown".to_string(), address_key)
}

pub struct MachineAuth(pub Id);

pub struct DeviceAuth(pub Id);

pub struct AccountAuth(pub String);

pub struct SameOrigin;

pub struct Anonymous {
    pub address: String,
    pub bearer: Option<String>,
}

impl FromRequestParts<Arc<Hub>> for MachineAuth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        hub: &Arc<Hub>,
    ) -> Result<MachineAuth, ApiError> {
        let address = client_address(parts, hub);
        hub.check_anonymous(&address)?;
        let token = bearer(&parts.headers).filter(|token| token.starts_with(MACHINE_TOKEN_PREFIX));
        let machine = match token {
            Some(token) => hub.store().machine_by_token(&digest(token))?,
            None => None,
        };
        let Some(machine) = machine else {
            return Err(hub.refuse_anonymous(&address));
        };
        hub.limit(&format!("token:{machine}"))?;
        hub.store().touch_machine(&machine, crate::now())?;
        Ok(MachineAuth(machine))
    }
}

impl FromRequestParts<Arc<Hub>> for DeviceAuth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, hub: &Arc<Hub>) -> Result<DeviceAuth, ApiError> {
        let address = client_address(parts, hub);
        hub.check_anonymous(&address)?;
        let Some(device) = device_for(hub, bearer(&parts.headers))? else {
            return Err(hub.refuse_anonymous(&address));
        };
        hub.limit(&format!("token:{device}"))?;
        hub.store().touch_device(&device, crate::now())?;
        Ok(DeviceAuth(device))
    }
}

pub fn device_for(hub: &Hub, token: Option<&str>) -> Result<Option<Id>, ApiError> {
    match token.filter(|token| token.starts_with(DEVICE_TOKEN_PREFIX)) {
        Some(token) => Ok(hub.store().device_by_token(&digest(token))?),
        None => Ok(None),
    }
}

impl FromRequestParts<Arc<Hub>> for AccountAuth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        hub: &Arc<Hub>,
    ) -> Result<AccountAuth, ApiError> {
        let address = client_address(parts, hub);
        hub.check_anonymous(&address)?;
        let account = match session_cookie(&parts.headers) {
            Some(token) => hub.store().session_account(&digest(token), crate::now())?,
            None => None,
        };
        let Some(account) = account else {
            return Err(hub.refuse_anonymous(&address));
        };
        hub.limit(&format!("account:{account}"))?;
        Ok(AccountAuth(account))
    }
}

impl FromRequestParts<Arc<Hub>> for SameOrigin {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, hub: &Arc<Hub>) -> Result<SameOrigin, ApiError> {
        let origin = parts
            .headers
            .get(ORIGIN)
            .and_then(|value| value.to_str().ok());
        if origin == Some(hub.origin()) {
            Ok(SameOrigin)
        } else {
            Err(ApiError::forbidden("cross-origin"))
        }
    }
}

impl FromRequestParts<Arc<Hub>> for Anonymous {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, hub: &Arc<Hub>) -> Result<Anonymous, ApiError> {
        let address = client_address(parts, hub);
        hub.check_anonymous(&address)?;
        Ok(Anonymous {
            address,
            bearer: bearer(&parts.headers).map(str::to_string),
        })
    }
}
