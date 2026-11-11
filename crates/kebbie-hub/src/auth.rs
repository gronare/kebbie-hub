use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::HeaderMap;
use axum::http::header::{AUTHORIZATION, COOKIE, ORIGIN};
use axum::http::request::Parts;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use kebbie_hub_protocol::Id;
use sha2::{Digest, Sha256};

use crate::error::ApiError;
use crate::limit::address_key;
use crate::store::{Access, Session};
use crate::{Hub, TrustedProxy};

pub const MACHINE_TOKEN_PREFIX: &str = "khm_";
pub const SESSION_TOKEN_PREFIX: &str = "khs_";
pub const SETUP_TOKEN_PREFIX: &str = "kho_";
pub const DEVICE_CODE_PREFIX: &str = "khc_";
pub const SESSION_COOKIE: &str = "__Host-kebbie_session";
pub const DEV_SESSION_COOKIE: &str = "kebbie_session";

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

pub fn session_cookie<'a>(hub: &Hub, headers: &'a HeaderMap) -> Option<&'a str> {
    let wanted = hub.session_cookie_name();
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == wanted)
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

pub struct AccountAuth(pub String);

pub struct SameOrigin;

pub struct FreshAccount(pub String);

// A session a passkey opened; one from email or GitHub reaches the account but not what needs the account key.
pub struct KeyAccount(pub String);

pub struct SessionAuth(pub Session);

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

impl FromRequestParts<Arc<Hub>> for AccountAuth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        hub: &Arc<Hub>,
    ) -> Result<AccountAuth, ApiError> {
        signed_in(parts, hub).map(|session| AccountAuth(session.account))
    }
}

fn signed_in(parts: &Parts, hub: &Arc<Hub>) -> Result<Session, ApiError> {
    let address = client_address(parts, hub);
    hub.check_anonymous(&address)?;
    let session = match session_cookie(hub, &parts.headers) {
        Some(token) => hub.store().session(&digest(token), crate::now())?,
        None => None,
    };
    let Some(session) = session else {
        return Err(hub.refuse_anonymous(&address));
    };
    hub.limit(&format!("account:{}", session.account))?;
    Ok(session)
}

pub fn passkey_required() -> ApiError {
    ApiError::forbidden("passkey-required")
}

impl FromRequestParts<Arc<Hub>> for KeyAccount {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, hub: &Arc<Hub>) -> Result<KeyAccount, ApiError> {
        let session = signed_in(parts, hub)?;
        if session.access != Access::Key {
            return Err(passkey_required());
        }
        Ok(KeyAccount(session.account))
    }
}

impl FromRequestParts<Arc<Hub>> for SessionAuth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, hub: &Arc<Hub>) -> Result<SessionAuth, ApiError> {
        signed_in(parts, hub).map(SessionAuth)
    }
}

impl FromRequestParts<Arc<Hub>> for FreshAccount {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        hub: &Arc<Hub>,
    ) -> Result<FreshAccount, ApiError> {
        let session = signed_in(parts, hub)?;
        if session.access != Access::Key {
            return Err(passkey_required());
        }
        if crate::now().saturating_sub(session.signed_in_at) > hub.config().fresh_session_ttl {
            return Err(ApiError::new(
                axum::http::StatusCode::UNAUTHORIZED,
                "reauth-required",
            ));
        }
        Ok(FreshAccount(session.account))
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
