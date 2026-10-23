use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit, Mac};
use kelpie_hub_api::{
    Bytes32, ReceivedReport, ReportAccepted, ReportChallenge, ReportInbox, ReportUpload,
    SignalUpload, SignalsAccepted,
};
use kelpie_hub_protocol::{meets, verify_report, verify_signals};
use serde::Deserialize;
use sha2::Sha256;

use crate::auth::{AdminAuth, Anonymous, random};
use crate::error::ApiError;
use crate::store::{KeyStatus, NewReport, NewSignals, ReportWrite, SignalWrite};
use crate::{Hub, MAX_STORED_INTEGER, now};

pub const INBOX_PAGE: u64 = 100;
const TAG_DOMAIN: &[u8] = b"kelpie-report-challenge-v1";

fn mac(secret: &[u8; 32], nonce: &[u8; 32], difficulty: u8, expires: u64) -> Hmac<Sha256> {
    let mut mac =
        <Hmac<Sha256> as KeyInit>::new_from_slice(secret).expect("HMAC takes a key of any length");
    mac.update(TAG_DOMAIN);
    mac.update(nonce);
    mac.update(&[difficulty]);
    mac.update(&expires.to_be_bytes());
    mac
}

fn genuine(secret: &[u8; 32], challenge: &ReportChallenge) -> bool {
    mac(
        secret,
        &challenge.nonce.0,
        challenge.difficulty,
        challenge.expires,
    )
    .verify_slice(&challenge.tag.0)
    .is_ok()
}

impl Hub {
    pub fn report_challenge(&self, at: u64) -> ReportChallenge {
        let nonce = random::<32>();
        let difficulty = self.config.report_difficulty;
        let expires = at + self.config.report_challenge_ttl;
        ReportChallenge {
            nonce: Bytes32(nonce),
            difficulty,
            expires,
            tag: Bytes32(
                mac(&self.challenge_secret, &nonce, difficulty, expires)
                    .finalize()
                    .into_bytes()
                    .into(),
            ),
        }
    }
}

pub async fn challenge(
    State(hub): State<Arc<Hub>>,
    client: Anonymous,
) -> Result<Json<ReportChallenge>, ApiError> {
    hub.limit_anonymous(&client.address)?;
    Ok(Json(hub.report_challenge(now())))
}

// Every check runs before the key's standing is looked up, so a blocked key sees what an allowed one sees.
pub async fn post_report(
    State(hub): State<Arc<Hub>>,
    client: Anonymous,
    Json(upload): Json<ReportUpload>,
) -> Result<(StatusCode, Json<ReportAccepted>), ApiError> {
    if !hub
        .report_address_limiter
        .allow(&format!("address:{}", client.address))
    {
        return Err(ApiError::rate_limited());
    }
    let at = now();
    let challenge = &upload.challenge;
    if !genuine(&hub.challenge_secret, challenge) {
        return Err(ApiError::bad_request("bad-challenge"));
    }
    if at >= challenge.expires {
        return Err(ApiError::bad_request("expired-challenge"));
    }
    upload.report.check().map_err(|error| match error {
        kelpie_hub_protocol::BugReportError::TooLarge => {
            ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "too-large")
        }
        other => ApiError::bad_request(other.key()),
    })?;
    if !meets(
        &challenge.nonce.0,
        &upload.report.digest(),
        upload.counter,
        challenge.difficulty,
    ) {
        return Err(ApiError::bad_request("insufficient-work"));
    }
    if !verify_report(
        &upload.key.0,
        &challenge.nonce.0,
        &upload.report,
        &upload.signature.0,
    ) {
        return Err(ApiError::bad_request("bad-signature"));
    }
    if !hub
        .report_key_limiter
        .allow(&URL_SAFE_NO_PAD.encode(upload.key.0))
    {
        return Err(ApiError::rate_limited());
    }
    let mut store = hub.store();
    if !store.spend_challenge(&challenge.nonce.0, challenge.expires)? {
        return Err(ApiError::conflict("challenge-used"));
    }
    let written = store.store_report(&NewReport {
        key: &upload.key.0,
        report: &upload.report,
        now: at,
        max_quarantined: hub.config.max_quarantined,
        max_quarantined_per_key: hub.config.max_quarantined_per_key,
    })?;
    let (id, outcome) = match written {
        ReportWrite::Stored(id, status) => (id, status.name()),
        ReportWrite::Dropped(id, KeyStatus::Blocked) => (id, "dropped-blocked"),
        ReportWrite::Dropped(id, _) => (id, "dropped-quarantine-full"),
    };
    store.log(
        at,
        "report.received",
        None,
        None,
        Some(&format!(
            "id={id} {outcome} bytes={}",
            upload.report.text.len()
        )),
    )?;
    Ok((StatusCode::ACCEPTED, Json(ReportAccepted { report: id })))
}

// The same order as a report's checks, and a blocked, repeating or flooding sender gets the same answer as a counted one.
pub async fn post_signals(
    State(hub): State<Arc<Hub>>,
    client: Anonymous,
    Json(upload): Json<SignalUpload>,
) -> Result<(StatusCode, Json<SignalsAccepted>), ApiError> {
    if !hub
        .report_address_limiter
        .allow(&format!("address:{}", client.address))
    {
        return Err(ApiError::rate_limited());
    }
    let at = now();
    let challenge = &upload.challenge;
    if !genuine(&hub.challenge_secret, challenge) {
        return Err(ApiError::bad_request("bad-challenge"));
    }
    if at >= challenge.expires {
        return Err(ApiError::bad_request("expired-challenge"));
    }
    upload
        .signals
        .check()
        .map_err(|error| ApiError::bad_request(error.key()))?;
    if !meets(
        &challenge.nonce.0,
        &upload.signals.digest(),
        upload.counter,
        challenge.difficulty,
    ) {
        return Err(ApiError::bad_request("insufficient-work"));
    }
    if !verify_signals(
        &upload.key.0,
        &challenge.nonce.0,
        &upload.signals,
        &upload.signature.0,
    ) {
        return Err(ApiError::bad_request("bad-signature"));
    }
    if !hub
        .report_key_limiter
        .allow(&URL_SAFE_NO_PAD.encode(upload.key.0))
    {
        return Err(ApiError::rate_limited());
    }
    let mut store = hub.store();
    if !store.spend_challenge(&challenge.nonce.0, challenge.expires)? {
        return Err(ApiError::conflict("challenge-used"));
    }
    let day = at / 86_400;
    let written = if hub.signal_batch_allowed(&client.address, day) {
        store.store_signals(&NewSignals {
            key: &upload.key.0,
            signals: &upload.signals,
            day,
            max_batches_per_key: hub.config.max_signal_batches_per_key,
            max_unvetted_rows: hub.config.max_unvetted_signal_rows,
        })?
    } else {
        SignalWrite::Dropped("dropped-address")
    };
    let outcome = match written {
        SignalWrite::Counted { vetted: true } => "counted",
        SignalWrite::Counted { vetted: false } => "counted-unvetted",
        SignalWrite::Dropped(why) => why,
    };
    let rows = upload.signals.counts.len();
    store.log(
        at,
        "signals.received",
        None,
        None,
        Some(&format!("{outcome} rows={rows}")),
    )?;
    Ok((StatusCode::ACCEPTED, Json(SignalsAccepted { rows })))
}

#[derive(Deserialize)]
pub struct InboxQuery {
    #[serde(default)]
    after: u64,
}

pub async fn inbox(
    State(hub): State<Arc<Hub>>,
    _admin: AdminAuth,
    Query(query): Query<InboxQuery>,
) -> Result<Json<ReportInbox>, ApiError> {
    if query.after > MAX_STORED_INTEGER {
        return Err(ApiError::bad_request("after-out-of-range"));
    }
    let store = hub.store();
    let reports = store
        .reports(KeyStatus::Allowed, query.after, true, INBOX_PAGE)?
        .into_iter()
        .map(|stored| ReceivedReport {
            id: stored.id,
            key: Bytes32(stored.key),
            received_at: stored.received_at,
            report: stored.report,
        })
        .collect();
    Ok(Json(ReportInbox {
        reports,
        quarantined: store.quarantined_count()?,
    }))
}

pub async fn ack(
    State(hub): State<Arc<Hub>>,
    _admin: AdminAuth,
    Path(id): Path<u64>,
) -> Result<StatusCode, ApiError> {
    if id > MAX_STORED_INTEGER {
        return Err(ApiError::not_found());
    }
    if hub.store().ack_report(id, now())? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found())
    }
}
