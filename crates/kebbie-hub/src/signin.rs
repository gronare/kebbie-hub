use kebbie_hub_api::{AccountView, SignupMode, normalize_code};
use webauthn_rs::prelude::Uuid;

use crate::account::{ACCOUNT_PREFIX, account_view, generated_handle, new_id, session_set_cookie};
use crate::auth::{SESSION_TOKEN_PREFIX, digest, new_token};
use crate::error::ApiError;
use crate::store::{Access, KeylessSignUp, SignUpOutcome};
use crate::{Hub, now};

pub struct KeylessSignIn {
    pub account: AccountView,
    pub set_cookie: String,
    pub created: bool,
}

fn opened(hub: &Hub, account: &str, session: &str, created: bool) -> Result<KeylessSignIn, ApiError> {
    Ok(KeylessSignIn {
        account: account_view(hub, account, Access::AccountOnly)?,
        set_cookie: session_set_cookie(hub, session, hub.config().session_ttl),
        created,
    })
}

// For an embedder's own sign-in route: the caller has proven who the person is, and the session it opens reaches the account but never its key.
pub fn sign_in_without_key(hub: &Hub, account: &str, via: &str) -> Result<KeylessSignIn, ApiError> {
    let session = new_token(SESSION_TOKEN_PREFIX);
    let at = now();
    {
        let store = hub.store();
        if store.account(account)?.is_none_or(|row| row.deactivated_at.is_some()) {
            return Err(ApiError::forbidden("deactivated"));
        }
        store.create_session(&digest(&session), account, at, at + hub.config().session_ttl, Access::AccountOnly)?;
        store.log(at, "account.logged-in", None, Some(&format!("{account} via={via}")))?;
    }
    opened(hub, account, &session, false)
}

// Creates an account from a pending `kebbie hub connect` code on a hub with open sign-up; the person adds a passkey next, which makes the account key.
pub fn sign_up_without_key(hub: &Hub, connect_code: &str, via: &str) -> Result<KeylessSignIn, ApiError> {
    if hub.config().signup != SignupMode::Open {
        return Err(ApiError::forbidden("signup-closed"));
    }
    let code = normalize_code(connect_code).ok_or_else(|| ApiError::new(axum::http::StatusCode::NOT_FOUND, "unknown-code"))?;
    let account = format!("{ACCOUNT_PREFIX}{}", new_id());
    let session = new_token(SESSION_TOKEN_PREFIX);
    let at = now();
    {
        let mut store = hub.store();
        let outcome = store.sign_up_keyless(&KeylessSignUp {
            invite_hash: &digest(&code),
            connect: true,
            account: &account,
            handle: &generated_handle(),
            user_handle: *Uuid::new_v4().as_bytes(),
            github: None,
            session_hash: &digest(&session),
            session_expires_at: at + hub.config().session_ttl,
            now: at,
        })?;
        match outcome {
            SignUpOutcome::Created(kind) => {
                store.log(at, "account.created", None, Some(&format!("{account} by={} via={via}", kind.name())))?;
            }
            SignUpOutcome::InviteSpent => return Err(ApiError::new(axum::http::StatusCode::NOT_FOUND, "unknown-code")),
            _ => return Err(ApiError::internal()),
        }
    }
    opened(hub, &account, &session, true)
}
