use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::header::CACHE_CONTROL;
use axum::response::{IntoResponse, Redirect, Response};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use crate::auth::AccountAuth;
use crate::error::ApiError;
use crate::{Hub, now};

pub const HANDOFF_SECRET_ENV: &str = "KEBBIE_HUB_BILLING_HANDOFF_SECRET";
pub const HANDOFF_DOMAIN: &str = "kebbie-billing-handoff-v1";
// A minute short of the verifier's ten-minute limit, so a verifier whose clock runs up to a minute behind the hub's still takes the link.
pub const HANDOFF_SECONDS: u64 = 540;

#[derive(Clone)]
pub struct HandoffSecret([u8; 32]);

impl HandoffSecret {
    pub fn parse(text: &str) -> Result<HandoffSecret, String> {
        let bytes = URL_SAFE_NO_PAD
            .decode(text.trim().trim_end_matches('='))
            .map_err(|_| "the billing handoff secret is not base64url".to_string())?;
        let bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| "the billing handoff secret must be 32 bytes".to_string())?;
        Ok(HandoffSecret(bytes))
    }
}

impl std::fmt::Debug for HandoffSecret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("HandoffSecret(…)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BillingPage {
    Checkout,
    Portal,
}

impl BillingPage {
    pub fn parse(purpose: &str) -> Option<BillingPage> {
        [BillingPage::Checkout, BillingPage::Portal]
            .into_iter()
            .find(|page| page.purpose() == purpose)
    }

    pub fn purpose(self) -> &'static str {
        match self {
            BillingPage::Checkout => "checkout",
            BillingPage::Portal => "portal",
        }
    }

    pub fn path(self) -> &'static str {
        match self {
            BillingPage::Checkout => "checkout",
            BillingPage::Portal => "billing_portal",
        }
    }
}

pub fn signature(secret: &HandoffSecret, page: BillingPage, account: &str, expires: u64) -> String {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(&secret.0)
        .expect("HMAC takes a key of any length");
    mac.update(HANDOFF_DOMAIN.as_bytes());
    mac.update(page.purpose().as_bytes());
    mac.update(b"\0");
    mac.update(account.as_bytes());
    mac.update(b"\0");
    mac.update(expires.to_string().as_bytes());
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

pub fn handoff_link(
    base: &str,
    secret: &HandoffSecret,
    page: BillingPage,
    account: &str,
    expires: u64,
) -> String {
    format!(
        "{}/{}?account={account}&expires={expires}&signature={}",
        base.trim_end_matches('/'),
        page.path(),
        signature(secret, page, account, expires)
    )
}

// The session cookie is SameSite=Strict, so a link opened from another site or app arrives without it and goes through the sign-in page, which comes back here.
pub async fn handoff(
    State(hub): State<Arc<Hub>>,
    signed_in: Result<AccountAuth, ApiError>,
    Path(purpose): Path<String>,
) -> Result<Response, ApiError> {
    let page = BillingPage::parse(&purpose).ok_or_else(ApiError::not_found)?;
    let (base, secret) = hub.billing_handoff().ok_or_else(ApiError::not_found)?;
    let account = match signed_in {
        Ok(AccountAuth(account)) => account,
        Err(refused) if refused.code() == "unauthorized" => {
            let sign_in = format!("/?next=%2Fv1%2Fa%2Fbilling%2F{}", page.purpose());
            return Ok(([(CACHE_CONTROL, "no-store")], Redirect::to(&sign_in)).into_response());
        }
        Err(refused) => return Err(refused),
    };
    let link = handoff_link(base, secret, page, &account, now() + HANDOFF_SECONDS);
    Ok(([(CACHE_CONTROL, "no-store")], Redirect::to(&link)).into_response())
}
