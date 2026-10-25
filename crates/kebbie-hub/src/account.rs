use std::sync::Arc;

use axum::Json;
use axum::extract::Query;
use axum::extract::{Path, State};
use axum::http::header::SET_COOKIE;
use axum::http::{HeaderMap, StatusCode};
use axum::response::AppendHeaders;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use std::collections::BTreeMap;

use kebbie_hub_api::{
    AccountKeySetup, AccountView, Bytes32, CreationCeremony, DeviceRegistered, DeviceRequest,
    DeviceView, Devices, EmailSignup, HubInfo, LoginBegin, LoginFinish, PasskeyView, Passkeys,
    RegisterBegin, RegisterFinish, RequestCeremony, SignedIn, SignupMode, Unlocking,
    VerifiedEmail, WrapperUpload, normalize_code,
};
use kebbie_hub_protocol::{Id, Prefix, WRAPPED_LEN};
use serde::Serialize;
use serde::de::DeserializeOwned;
use webauthn_rs::prelude::{
    AuthenticationResult, CreationChallengeResponse, CredentialID, DiscoverableAuthentication,
    DiscoverableKey, Passkey, PasskeyAuthentication, PasskeyRegistration, PublicKeyCredential,
    Uuid, WebauthnError,
};
use webauthn_rs_proto::{
    AuthenticatorSelectionCriteria, ResidentKeyRequirement, UserVerificationPolicy,
};

use crate::auth::{
    AccountAuth, Anonymous, DEVICE_TOKEN_PREFIX, FreshAccount, SESSION_COOKIE, SESSION_TOKEN_PREFIX,
    SETUP_TOKEN_PREFIX, SameOrigin, VERIFY_TOKEN_PREFIX, digest, new_token, random, session_cookie,
};
use crate::error::ApiError;
use crate::mail::{Mail, plausible_address};
use crate::store::{
    Ceremony, KeySet, Purpose, Removal, SignUp, SignUpOutcome, StoredPasskey, Ticket,
};
use crate::{Hub, clean_name, now};

const MAX_HANDLE_CHARS: usize = 32;
const ACCOUNT_PREFIX: &str = "acc_";

type WithCookie<T> = (
    AppendHeaders<[(axum::http::HeaderName, String); 1]>,
    Json<T>,
);

pub fn counter_regressed(stored: u32, presented: u32) -> bool {
    (stored != 0 || presented != 0) && presented <= stored
}

pub fn normalize_handle(typed: &str) -> Option<String> {
    let handle = typed.trim().to_ascii_lowercase();
    let starts_with_letter = handle
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_lowercase());
    let allowed = handle
        .chars()
        .all(|char| char.is_ascii_lowercase() || char.is_ascii_digit() || char == '-');
    (starts_with_letter && allowed && handle.len() <= MAX_HANDLE_CHARS).then_some(handle)
}

fn new_id() -> String {
    URL_SAFE_NO_PAD.encode(random::<16>())
}

fn refused(error: WebauthnError) -> ApiError {
    eprintln!("kebbie-hub: passkey refused: {error}");
    ApiError::bad_request("passkey-refused")
}

// Sign-in names no account, so every passkey the hub makes must be one the authenticator can offer by itself.
fn discoverable(mut options: CreationChallengeResponse) -> CreationChallengeResponse {
    options.public_key.authenticator_selection = Some(AuthenticatorSelectionCriteria {
        authenticator_attachment: None,
        resident_key: Some(ResidentKeyRequirement::Required),
        require_resident_key: true,
        user_verification: UserVerificationPolicy::Required,
    });
    options
}

fn to_json<T: Serialize>(value: &T) -> Result<String, ApiError> {
    serde_json::to_string(value).map_err(|_| ApiError::internal())
}

fn from_json<T: DeserializeOwned>(text: &str) -> Result<T, ApiError> {
    serde_json::from_str(text).map_err(|_| ApiError::internal())
}

fn optional_name(name: Option<&str>) -> Result<Option<String>, ApiError> {
    name.map(|text| {
        clean_name(text)
            .map(str::to_string)
            .ok_or_else(|| ApiError::bad_request("bad-name"))
    })
    .transpose()
}

fn session_set_cookie(token: &str, max_age: u64) -> String {
    format!(
        "{SESSION_COOKIE}={token}; Path=/; Max-Age={max_age}; HttpOnly; Secure; SameSite=Strict"
    )
}

fn with_session<T>(hub: &Hub, token: &str, body: T) -> WithCookie<T> {
    (
        AppendHeaders([(
            SET_COOKIE,
            session_set_cookie(token, hub.config().session_ttl),
        )]),
        Json(body),
    )
}

fn passkey_view(stored: &StoredPasskey) -> PasskeyView {
    PasskeyView {
        passkey: URL_SAFE_NO_PAD.encode(&stored.credential_id),
        name: stored.name.clone(),
        created_at: stored.created_at,
        last_used_at: stored.last_used_at,
        unlocks: stored.unlocks,
    }
}

fn unlocking(hub: &Hub, credential: &[u8], prf_salt: [u8; 32]) -> Result<Unlocking, ApiError> {
    let wrapped = hub.store().wrapper(credential)?;
    Ok(Unlocking {
        credential: URL_SAFE_NO_PAD.encode(credential),
        prf_salt: Bytes32(prf_salt),
        wrapped: wrapped.map(|(_, wrapped)| URL_SAFE_NO_PAD.encode(wrapped)),
    })
}

fn signed_in(hub: &Hub, account: &str, credential: &[u8], prf_salt: [u8; 32]) -> Result<SignedIn, ApiError> {
    Ok(SignedIn {
        account: account_view(hub, account)?,
        unlock: unlocking(hub, credential, prf_salt)?,
    })
}

fn salts_of(stored: &[StoredPasskey]) -> BTreeMap<String, Bytes32> {
    stored
        .iter()
        .map(|stored| (URL_SAFE_NO_PAD.encode(&stored.credential_id), Bytes32(stored.prf_salt)))
        .collect()
}

fn wrapped_of(text: &str) -> Result<Vec<u8>, ApiError> {
    URL_SAFE_NO_PAD
        .decode(text)
        .ok()
        .filter(|wrapped| wrapped.len() == WRAPPED_LEN)
        .ok_or(ApiError::bad_request("bad-wrapped"))
}

pub fn account_view(hub: &Hub, account: &str) -> Result<AccountView, ApiError> {
    let row = hub
        .store()
        .account(account)?
        .ok_or_else(ApiError::unauthorized)?;
    let plan = hub.plan_view(&row, now());
    let key = hub.store().account_key(account)?;
    Ok(AccountView {
        key,
        account: row.id,
        handle: row.handle,
        created_at: row.created_at,
        owner: row.owner,
        email: row.email,
        billing_url: hub.config().billing_url.clone().filter(|_| !plan.entitled),
        plan,
    })
}

fn ticket_of(request: &RegisterBegin) -> Option<(Ticket, Vec<u8>)> {
    match (&request.setup, &request.verification) {
        (Some(setup), None) => setup
            .trim()
            .starts_with(SETUP_TOKEN_PREFIX)
            .then(|| (Ticket::Owner, digest(setup.trim()))),
        (None, Some(verification)) => verification
            .trim()
            .starts_with(VERIFY_TOKEN_PREFIX)
            .then(|| (Ticket::Email, digest(verification.trim()))),
        (None, None) => {
            normalize_code(&request.invite).map(|invite| (Ticket::Invite, digest(&invite)))
        }
        (Some(_), Some(_)) => None,
    }
}

fn save_ceremony(
    hub: &Hub,
    purpose: Purpose,
    account: Option<String>,
    state: String,
    signup: Option<(String, [u8; 16], Vec<u8>)>,
    prf_salt: Option<[u8; 32]>,
) -> Result<String, ApiError> {
    let id = new_id();
    let (handle, user_handle, invite_hash) = match signup {
        Some((handle, user, invite)) => (Some(handle), Some(user), Some(invite)),
        None => (None, None, None),
    };
    hub.store().save_ceremony(&Ceremony {
        id_hash: digest(&id),
        purpose,
        account,
        handle,
        user_handle,
        invite_hash,
        state,
        expires_at: now() + hub.config().ceremony_ttl,
        prf_salt,
    })?;
    Ok(id)
}

fn take_ceremony(hub: &Hub, id: &str, purpose: Purpose) -> Result<Ceremony, ApiError> {
    hub.store()
        .take_ceremony(&digest(id), purpose, now())?
        .ok_or_else(ApiError::unauthorized)
}

pub async fn register_begin(
    State(hub): State<Arc<Hub>>,
    _: SameOrigin,
    client: Anonymous,
    Json(request): Json<RegisterBegin>,
) -> Result<Json<CreationCeremony>, ApiError> {
    hub.limit_anonymous(&client.address)?;
    let handle = normalize_handle(&request.handle).ok_or(ApiError::bad_request("bad-handle"))?;
    let (kind, invite_hash) = ticket_of(&request).ok_or_else(ApiError::unauthorized)?;
    {
        let store = hub.store();
        let (held, email) = store
            .ticket(&invite_hash, now())?
            .ok_or_else(ApiError::unauthorized)?;
        if held != kind {
            return Err(ApiError::unauthorized());
        }
        if kind == Ticket::Owner && store.account_count()? > 0 {
            return Err(ApiError::unauthorized());
        }
        if kind == Ticket::Email && hub.config().signup != SignupMode::Open {
            return Err(ApiError::forbidden("signup-closed"));
        }
        if let Some(email) = email
            && store.email_taken(&email)?
        {
            return Err(ApiError::conflict("email-taken"));
        }
        if store.handle_taken(&handle)? {
            return Err(ApiError::conflict("handle-taken"));
        }
    }
    let user = Uuid::new_v4();
    let (options, state) = hub
        .webauthn()
        .start_passkey_registration(user, &handle, &handle, None)
        .map_err(refused)?;
    let options = discoverable(options);
    let prf_salt = random::<32>();
    let ceremony = save_ceremony(
        &hub,
        Purpose::SignUp,
        None,
        to_json(&state)?,
        Some((handle, *user.as_bytes(), invite_hash)),
        Some(prf_salt),
    )?;
    Ok(Json(CreationCeremony {
        ceremony,
        options,
        prf_salt: Bytes32(prf_salt),
    }))
}

pub async fn register_finish(
    State(hub): State<Arc<Hub>>,
    _: SameOrigin,
    client: Anonymous,
    Json(request): Json<RegisterFinish>,
) -> Result<WithCookie<SignedIn>, ApiError> {
    hub.limit_anonymous(&client.address)?;
    let name = optional_name(request.name.as_deref())?;
    let ceremony = take_ceremony(&hub, &request.ceremony, Purpose::SignUp)?;
    let state: PasskeyRegistration = from_json(&ceremony.state)?;
    let passkey = hub
        .webauthn()
        .finish_passkey_registration(&request.credential, &state)
        .map_err(refused)?;
    let (Some(handle), Some(user_handle), Some(invite_hash), Some(prf_salt)) = (
        ceremony.handle,
        ceremony.user_handle,
        ceremony.invite_hash,
        ceremony.prf_salt,
    ) else {
        return Err(ApiError::internal());
    };
    let account = format!("{ACCOUNT_PREFIX}{}", new_id());
    let session = new_token(SESSION_TOKEN_PREFIX);
    let at = now();
    let mut store = hub.store();
    let outcome = store.sign_up(&SignUp {
        invite_hash: &invite_hash,
        account: &account,
        handle: &handle,
        user_handle,
        credential_id: passkey.cred_id().as_ref(),
        name: name.as_deref(),
        passkey: &to_json(&passkey)?,
        prf_salt,
        session_hash: &digest(&session),
        session_expires_at: at + hub.config().session_ttl,
        now: at,
    })?;
    let kind = match outcome {
        SignUpOutcome::Created(kind) => kind,
        SignUpOutcome::EmailTaken => return Err(ApiError::conflict("email-taken")),
        SignUpOutcome::InviteSpent => return Err(ApiError::unauthorized()),
        SignUpOutcome::HandleTaken => return Err(ApiError::conflict("handle-taken")),
        SignUpOutcome::CredentialTaken => return Err(ApiError::conflict("passkey-taken")),
    };
    store.log(
        at,
        "account.created",
        None,
        None,
        Some(&format!("{account} by={}", kind.name())),
    )?;
    drop(store);
    if kind == Ticket::Owner {
        hub.forget_setup();
    }
    let view = signed_in(&hub, &account, passkey.cred_id().as_ref(), prf_salt)?;
    Ok(with_session(&hub, &session, view))
}

pub async fn login_begin(
    State(hub): State<Arc<Hub>>,
    _: SameOrigin,
    client: Anonymous,
    Json(request): Json<LoginBegin>,
) -> Result<Json<RequestCeremony>, ApiError> {
    hub.limit_anonymous(&client.address)?;
    let handle = request.handle.filter(|typed| !typed.trim().is_empty());
    let credential = request.credential.filter(|typed| !typed.trim().is_empty());
    let (account, stored) = match (handle, credential) {
        (None, None) => {
            let (mut options, state) = hub
                .webauthn()
                .start_discoverable_authentication()
                .map_err(refused)?;
            options.mediation = None;
            let ceremony =
                save_ceremony(&hub, Purpose::LogIn, None, to_json(&state)?, None, None)?;
            return Ok(Json(RequestCeremony {
                ceremony,
                options,
                prf_salts: BTreeMap::new(),
            }));
        }
        (Some(typed), None) => {
            let handle = normalize_handle(&typed).ok_or_else(ApiError::unauthorized)?;
            let store = hub.store();
            let account = store
                .account_by_handle(&handle)?
                .ok_or_else(ApiError::unauthorized)?;
            let stored = store.passkeys(&account)?;
            (account, stored)
        }
        (None, Some(typed)) => {
            let credential = URL_SAFE_NO_PAD
                .decode(typed.trim())
                .map_err(|_| ApiError::unauthorized())?;
            let store = hub.store();
            let account = store
                .passkey_account(&credential)?
                .ok_or_else(ApiError::unauthorized)?;
            let stored = store
                .passkeys(&account)?
                .into_iter()
                .filter(|stored| stored.credential_id == credential)
                .collect();
            (account, stored)
        }
        (Some(_), Some(_)) => return Err(ApiError::bad_request("handle-or-credential")),
    };
    if hub
        .store()
        .account(&account)?
        .is_none_or(|row| row.deactivated_at.is_some())
    {
        return Err(ApiError::unauthorized());
    }
    let passkeys = stored
        .iter()
        .map(|stored| from_json::<Passkey>(&stored.passkey))
        .collect::<Result<Vec<_>, _>>()?;
    if passkeys.is_empty() {
        return Err(ApiError::unauthorized());
    }
    let (options, state) = hub
        .webauthn()
        .start_passkey_authentication(&passkeys)
        .map_err(refused)?;
    let ceremony = save_ceremony(
        &hub,
        Purpose::LogIn,
        Some(account),
        to_json(&state)?,
        None,
        None,
    )?;
    Ok(Json(RequestCeremony {
        ceremony,
        options,
        prf_salts: salts_of(&stored),
    }))
}

fn unknown_passkey() -> ApiError {
    ApiError::new(StatusCode::UNAUTHORIZED, "unknown-passkey")
}

fn passkey_failed(error: WebauthnError) -> ApiError {
    eprintln!("kebbie-hub: passkey refused: {error}");
    ApiError::unauthorized()
}

// The authenticator named the account through the credential's user handle, so the account is looked up from it and only its own passkeys are allowed.
fn discovered(
    hub: &Hub,
    credential: &PublicKeyCredential,
    state: &str,
) -> Result<(String, AuthenticationResult), ApiError> {
    let state: DiscoverableAuthentication = from_json(state)?;
    let (user, _) = hub
        .webauthn()
        .identify_discoverable_authentication(credential)
        .map_err(|_| unknown_passkey())?;
    let (account, stored) = {
        let store = hub.store();
        let account = store
            .account_by_user_handle(user.as_bytes())?
            .ok_or_else(unknown_passkey)?;
        if store
            .account(&account)?
            .is_none_or(|row| row.deactivated_at.is_some())
        {
            return Err(ApiError::unauthorized());
        }
        (account.clone(), store.passkeys(&account)?)
    };
    let keys = stored
        .iter()
        .map(|stored| from_json::<Passkey>(&stored.passkey).map(|passkey| DiscoverableKey::from(&passkey)))
        .collect::<Result<Vec<_>, _>>()?;
    let result = hub
        .webauthn()
        .finish_discoverable_authentication(credential, state, &keys)
        .map_err(passkey_failed)?;
    Ok((account, result))
}

pub async fn login_finish(
    State(hub): State<Arc<Hub>>,
    _: SameOrigin,
    client: Anonymous,
    Json(request): Json<LoginFinish>,
) -> Result<WithCookie<SignedIn>, ApiError> {
    hub.limit_anonymous(&client.address)?;
    let ceremony = take_ceremony(&hub, &request.ceremony, Purpose::LogIn)?;
    let (account, result) = match ceremony.account {
        Some(account) => {
            let state: PasskeyAuthentication = from_json(&ceremony.state)?;
            let result = hub
                .webauthn()
                .finish_passkey_authentication(&request.credential, &state)
                .map_err(passkey_failed)?;
            (account, result)
        }
        None => discovered(&hub, &request.credential, &ceremony.state)?,
    };
    let used: &[u8] = result.cred_id().as_ref();
    let at = now();
    let session = new_token(SESSION_TOKEN_PREFIX);
    let prf_salt = {
        let store = hub.store();
        let stored = store
            .passkeys(&account)?
            .into_iter()
            .find(|stored| stored.credential_id == used)
            .ok_or_else(ApiError::unauthorized)?;
        if counter_regressed(stored.counter, result.counter()) {
            store.log(at, "passkey.clone-suspected", None, None, Some(&account))?;
            return Err(ApiError::unauthorized());
        }
        let mut passkey: Passkey = from_json(&stored.passkey)?;
        let updated = match passkey.update_credential(&result) {
            Some(true) => Some(to_json(&passkey)?),
            _ => None,
        };
        store.use_passkey(used, updated.as_deref(), result.counter(), at)?;
        store.create_session(
            &digest(&session),
            &account,
            at,
            at + hub.config().session_ttl,
        )?;
        store.log(at, "account.logged-in", None, None, Some(&account))?;
        stored.prf_salt
    };
    let view = signed_in(&hub, &account, used, prf_salt)?;
    Ok(with_session(&hub, &session, view))
}

pub async fn logout(
    State(hub): State<Arc<Hub>>,
    _: SameOrigin,
    client: Anonymous,
    headers: HeaderMap,
) -> Result<
    (
        AppendHeaders<[(axum::http::HeaderName, String); 1]>,
        StatusCode,
    ),
    ApiError,
> {
    hub.limit_anonymous(&client.address)?;
    if let Some(token) = session_cookie(&headers) {
        hub.store().end_session(&digest(token))?;
    }
    Ok((
        AppendHeaders([(SET_COOKIE, session_set_cookie("", 0))]),
        StatusCode::NO_CONTENT,
    ))
}

pub async fn hub_info(
    State(hub): State<Arc<Hub>>,
    client: Anonymous,
) -> Result<Json<HubInfo>, ApiError> {
    hub.limit_anonymous(&client.address)?;
    Ok(Json(HubInfo {
        signup: hub.config().signup,
        plans: hub.config().plans,
        needs_setup: hub.store().account_count()? == 0,
    }))
}

pub async fn email_signup(
    State(hub): State<Arc<Hub>>,
    _: SameOrigin,
    client: Anonymous,
    Json(request): Json<EmailSignup>,
) -> Result<StatusCode, ApiError> {
    hub.limit_anonymous(&client.address)?;
    if hub.config().signup != SignupMode::Open {
        return Err(ApiError::forbidden("signup-closed"));
    }
    let email = request.email.trim().to_ascii_lowercase();
    if !plausible_address(&email) {
        return Err(ApiError::bad_request("bad-email"));
    }
    if !hub
        .email_limiter
        .allow(&format!("address:{}", client.address))
        || !hub.email_limiter.allow(&format!("email:{email}"))
    {
        return Err(ApiError::rate_limited());
    }
    let at = now();
    let token = new_token(VERIFY_TOKEN_PREFIX);
    {
        let store = hub.store();
        if store.email_taken(&email)? {
            store.log(at, "signup.email-held", None, None, None)?;
            return Ok(StatusCode::ACCEPTED);
        }
        store.add_ticket(
            &digest(&token),
            Ticket::Email,
            Some(&email),
            at + hub.config().verify_ttl,
        )?;
    }
    let mail = Mail {
        to: email,
        subject: "Finish signing up to kebbie hub".to_string(),
        text: format!(
            "Open this link to pick a handle and add a passkey:\n\n{}/signup?verify={token}\n\nThe link works once, for {} hours. If you did not ask for it, ignore this mail.",
            hub.origin(),
            hub.config().verify_ttl / 3600
        ),
    };
    if let Err(error) = hub.mailer.send(&mail).await {
        eprintln!("kebbie-hub: sign-up mail failed: {error}");
        hub.store()
            .log(at, "signup.mail-failed", None, None, None)?;
        return Err(ApiError::new(StatusCode::BAD_GATEWAY, "mail-failed"));
    }
    hub.store().log(at, "signup.mail-sent", None, None, None)?;
    Ok(StatusCode::ACCEPTED)
}

#[derive(serde::Deserialize)]
pub struct VerificationQuery {
    token: String,
}

pub async fn verification(
    State(hub): State<Arc<Hub>>,
    client: Anonymous,
    Query(query): Query<VerificationQuery>,
) -> Result<Json<VerifiedEmail>, ApiError> {
    hub.limit_anonymous(&client.address)?;
    let ticket = query
        .token
        .starts_with(VERIFY_TOKEN_PREFIX)
        .then(|| hub.store().ticket(&digest(&query.token), now()))
        .transpose()?
        .flatten();
    match ticket {
        Some((Ticket::Email, Some(email))) => Ok(Json(VerifiedEmail { email })),
        _ => Err(ApiError::unauthorized()),
    }
}

pub async fn account(
    State(hub): State<Arc<Hub>>,
    AccountAuth(account): AccountAuth,
) -> Result<Json<AccountView>, ApiError> {
    Ok(Json(account_view(&hub, &account)?))
}

pub async fn passkey_begin(
    State(hub): State<Arc<Hub>>,
    _: SameOrigin,
    AccountAuth(account): AccountAuth,
) -> Result<Json<CreationCeremony>, ApiError> {
    let (row, existing) = {
        let store = hub.store();
        let row = store
            .account(&account)?
            .ok_or_else(ApiError::unauthorized)?;
        (row, store.passkeys(&account)?)
    };
    let exclude: Vec<CredentialID> = existing
        .into_iter()
        .map(|stored| CredentialID::from(stored.credential_id))
        .collect();
    let (options, state) = hub
        .webauthn()
        .start_passkey_registration(
            Uuid::from_bytes(row.user_handle),
            &row.handle,
            &row.handle,
            Some(exclude),
        )
        .map_err(refused)?;
    let options = discoverable(options);
    let prf_salt = random::<32>();
    let ceremony = save_ceremony(
        &hub,
        Purpose::AddPasskey,
        Some(account),
        to_json(&state)?,
        None,
        Some(prf_salt),
    )?;
    Ok(Json(CreationCeremony {
        ceremony,
        options,
        prf_salt: Bytes32(prf_salt),
    }))
}

pub async fn passkey_finish(
    State(hub): State<Arc<Hub>>,
    _: SameOrigin,
    AccountAuth(account): AccountAuth,
    Json(request): Json<RegisterFinish>,
) -> Result<Json<PasskeyView>, ApiError> {
    let name = optional_name(request.name.as_deref())?;
    let ceremony = take_ceremony(&hub, &request.ceremony, Purpose::AddPasskey)?;
    if ceremony.account.as_deref() != Some(account.as_str()) {
        return Err(ApiError::unauthorized());
    }
    let state: PasskeyRegistration = from_json(&ceremony.state)?;
    let prf_salt = ceremony.prf_salt.ok_or_else(ApiError::internal)?;
    let passkey = hub
        .webauthn()
        .finish_passkey_registration(&request.credential, &state)
        .map_err(refused)?;
    let credential_id: &[u8] = passkey.cred_id().as_ref();
    let at = now();
    let store = hub.store();
    if !store.add_passkey(
        &account,
        credential_id,
        name.as_deref(),
        &to_json(&passkey)?,
        &prf_salt,
        at,
    )? {
        return Err(ApiError::conflict("passkey-taken"));
    }
    store.log(at, "passkey.added", None, None, Some(&account))?;
    Ok(Json(PasskeyView {
        passkey: URL_SAFE_NO_PAD.encode(credential_id),
        name,
        created_at: at,
        last_used_at: None,
        unlocks: false,
    }))
}

pub async fn passkeys(
    State(hub): State<Arc<Hub>>,
    AccountAuth(account): AccountAuth,
) -> Result<Json<Passkeys>, ApiError> {
    let passkeys = hub
        .store()
        .passkeys(&account)?
        .iter()
        .map(passkey_view)
        .collect();
    Ok(Json(Passkeys { passkeys }))
}

pub async fn remove_passkey(
    State(hub): State<Arc<Hub>>,
    _: SameOrigin,
    AccountAuth(account): AccountAuth,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let credential_id = URL_SAFE_NO_PAD
        .decode(&id)
        .map_err(|_| ApiError::not_found())?;
    match hub.store().remove_passkey(&account, &credential_id)? {
        Removal::Removed => Ok(StatusCode::NO_CONTENT),
        Removal::Last => Err(ApiError::conflict("last-passkey")),
        Removal::LastUnlocking => Err(ApiError::conflict("last-unlocking-passkey")),
        Removal::Missing => Err(ApiError::not_found()),
    }
}

pub async fn set_key(
    State(hub): State<Arc<Hub>>,
    _: SameOrigin,
    FreshAccount(account): FreshAccount,
    Json(request): Json<AccountKeySetup>,
) -> Result<StatusCode, ApiError> {
    if request.key.account.as_str() != account || request.key.version != 1 {
        return Err(ApiError::bad_request("bad-key"));
    }
    let credential = URL_SAFE_NO_PAD
        .decode(&request.credential)
        .map_err(|_| unknown_passkey_here())?;
    let wrapped = wrapped_of(&request.wrapped)?;
    if !request
        .key
        .verifies_wrapper(&credential, &wrapped, &request.signature.0)
    {
        return Err(ApiError::bad_request("bad-signature"));
    }
    let at = now();
    let mut store = hub.store();
    match store.set_account_key(&request.key, &credential, &wrapped, at)? {
        KeySet::Set => {}
        KeySet::KeyExists => return Err(ApiError::conflict("key-exists")),
        KeySet::UnknownPasskey => return Err(unknown_passkey_here()),
    }
    store.log(at, "account.key-set", None, None, Some(&account))?;
    Ok(StatusCode::NO_CONTENT)
}

fn unknown_passkey_here() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "unknown-passkey")
}

pub async fn set_wrapper(
    State(hub): State<Arc<Hub>>,
    _: SameOrigin,
    FreshAccount(account): FreshAccount,
    Path(id): Path<String>,
    Json(request): Json<WrapperUpload>,
) -> Result<StatusCode, ApiError> {
    let credential = URL_SAFE_NO_PAD
        .decode(&id)
        .map_err(|_| unknown_passkey_here())?;
    let wrapped = wrapped_of(&request.wrapped)?;
    let at = now();
    let store = hub.store();
    let key = store
        .account_key(&account)?
        .ok_or(ApiError::conflict("no-key"))?;
    if key.version != request.key_version {
        return Err(ApiError::conflict("key-version"));
    }
    if !key.verifies_wrapper(&credential, &wrapped, &request.signature.0) {
        return Err(ApiError::bad_request("bad-signature"));
    }
    if !store.set_wrapper(&account, &credential, key.version, &wrapped, at)? {
        return Err(unknown_passkey_here());
    }
    store.log(at, "passkey.wrapper-set", None, None, Some(&account))?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn register_device(
    State(hub): State<Arc<Hub>>,
    _: SameOrigin,
    AccountAuth(account): AccountAuth,
    Json(request): Json<DeviceRequest>,
) -> Result<Json<DeviceRegistered>, ApiError> {
    let name = optional_name(request.name.as_deref())?;
    let device = Id::new(Prefix::Device).expect("a fresh device id is valid");
    let token = new_token(DEVICE_TOKEN_PREFIX);
    let at = now();
    let store = hub.store();
    store.create_device(
        &device,
        &digest(&token),
        Some(&account),
        name.as_deref(),
        at,
    )?;
    store.log(at, "device.registered", None, Some(&device), Some(&account))?;
    Ok(Json(DeviceRegistered { device, token }))
}

pub async fn devices(
    State(hub): State<Arc<Hub>>,
    AccountAuth(account): AccountAuth,
) -> Result<Json<Devices>, ApiError> {
    let devices = hub
        .store()
        .devices_of_account(&account)?
        .into_iter()
        .map(|row| DeviceView {
            device: row.id,
            name: row.name,
            created_at: row.created_at,
            last_seen: row.last_seen,
            machines: row.machines,
        })
        .collect();
    Ok(Json(Devices { devices }))
}

pub async fn remove_device(
    State(hub): State<Arc<Hub>>,
    _: SameOrigin,
    AccountAuth(account): AccountAuth,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let device = Id::parse(&id).map_err(|_| ApiError::not_found())?;
    let at = now();
    {
        let store = hub.store();
        if !store.remove_account_device(&account, &device)? {
            return Err(ApiError::not_found());
        }
        store.log(at, "device.removed", None, Some(&device), Some(&account))?;
    }
    hub.wake_device(&device);
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::{counter_regressed, normalize_handle};

    #[test]
    fn a_counter_that_does_not_move_forward_is_a_clone_unless_both_are_zero() {
        assert!(!counter_regressed(0, 0));
        assert!(!counter_regressed(0, 1));
        assert!(!counter_regressed(4, 5));
        assert!(counter_regressed(5, 5));
        assert!(counter_regressed(5, 4));
        assert!(counter_regressed(5, 0));
    }

    #[test]
    fn handles_are_lowercase_words_that_start_with_a_letter() {
        assert_eq!(normalize_handle(" Carl ").as_deref(), Some("carl"));
        assert_eq!(normalize_handle("carl-2").as_deref(), Some("carl-2"));
        for refused in ["", "2carl", "carl green", "carl_", "ä", &"a".repeat(33)] {
            assert_eq!(normalize_handle(refused), None, "{refused:?}");
        }
    }
}
