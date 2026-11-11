use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::response::{IntoResponse, Response};
use kebbie_hub_protocol::{BLOB_OVERHEAD, Id, Prefix};

use crate::auth::{AccountAuth, MachineAuth};
use crate::error::ApiError;
use crate::store::BlobWrite;
use crate::{Hub, now};

fn blob_id(text: &str) -> Option<Id> {
    Id::parse(text).ok().filter(|id| id.prefix() == Prefix::Blob)
}

pub async fn put_blob(
    State(hub): State<Arc<Hub>>,
    MachineAuth(machine): MachineAuth,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    let id = blob_id(&id).ok_or(ApiError::bad_request("bad-id"))?;
    if body.len() <= BLOB_OVERHEAD {
        return Err(ApiError::bad_request("too-short"));
    }
    if Id::blob(&body) != id {
        return Err(ApiError::bad_request("id-mismatch"));
    }
    let at = now();
    let mut store = hub.store();
    match store.store_blob(&machine, &id, &body, at)? {
        BlobWrite::Renewed => {}
        BlobWrite::Stored => store.log(
            at,
            "blob.stored",
            Some(&machine),
            None,
            Some(&format!("bytes={}", body.len())),
        )?,
        BlobWrite::OverQuota => {
            return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "blob-quota"));
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn blob(
    State(hub): State<Arc<Hub>>,
    AccountAuth(account): AccountAuth,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id = blob_id(&id).ok_or_else(ApiError::not_found)?;
    let ciphertext = hub
        .store()
        .blob_for_account(&id, &account)?
        .ok_or_else(ApiError::not_found)?;
    let headers = [
        (CONTENT_TYPE, "application/octet-stream"),
        (CACHE_CONTROL, "private, no-store"),
    ];
    Ok((headers, ciphertext).into_response())
}
