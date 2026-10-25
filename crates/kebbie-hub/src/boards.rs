use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use kebbie_hub_api::{BoardEntry, Boards, Bytes64, SignedMachineList};
use kebbie_hub_protocol::{Envelope, Id};

use crate::auth::{AccountAuth, FreshAccount, SameOrigin};
use crate::error::ApiError;
use crate::store::ListWrite;
use crate::{Hub, now};

pub async fn boards(
    State(hub): State<Arc<Hub>>,
    AccountAuth(account): AccountAuth,
) -> Result<Json<Boards>, ApiError> {
    let reader = Id::parse(&account).map_err(|_| ApiError::internal())?;
    let (stored, list) = {
        let store = hub.store();
        (
            store.boards_for_account(&account)?,
            store.machine_list(&account)?,
        )
    };
    let boards = stored
        .into_iter()
        .map(|row| {
            let (rev, stored_at, envelope) = match row.board {
                Some((rev, stored_at, text)) => {
                    let envelope = Envelope::from_json(&text).map_err(|_| ApiError::internal())?;
                    (Some(rev), Some(stored_at), envelope.for_recipient(&reader))
                }
                None => (None, None, None),
            };
            Ok(BoardEntry {
                machine: row.machine,
                name: row.name,
                last_seen: row.last_seen,
                machine_keys: row.keys,
                rev,
                stored_at,
                envelope,
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    let list = list.map(|(list, signature)| SignedMachineList {
        list,
        signature: Bytes64(signature),
    });
    Ok(Json(Boards { boards, list }))
}

pub async fn put_machines(
    State(hub): State<Arc<Hub>>,
    _: SameOrigin,
    AccountAuth(account): AccountAuth,
    Json(request): Json<SignedMachineList>,
) -> Result<StatusCode, ApiError> {
    let mut store = hub.store();
    let key = store
        .account_key(&account)?
        .ok_or(ApiError::conflict("no-key"))?;
    if !key.verifies_list(&request.list, &request.signature.0) {
        return Err(ApiError::bad_request("bad-signature"));
    }
    match store.set_machine_list(&account, &request.list, &request.signature.0)? {
        ListWrite::Stored => {}
        ListWrite::Stale => return Err(ApiError::conflict("stale-list")),
        ListWrite::UnknownMachine => return Err(ApiError::bad_request("unknown-machine")),
    }
    store.log(
        now(),
        "machines.signed",
        None,
        None,
        Some(&format!("{account} counter={}", request.list.counter)),
    )?;
    drop(store);
    hub.wake_account(&account);
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_machine(
    State(hub): State<Arc<Hub>>,
    _: SameOrigin,
    FreshAccount(account): FreshAccount,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let machine = Id::parse(&id).map_err(|_| ApiError::not_found())?;
    let readers = {
        let store = hub.store();
        if store.machine_owner(&machine)?.as_deref() != Some(account.as_str()) {
            return Err(ApiError::not_found());
        }
        let listed = store
            .machine_list(&account)?
            .is_some_and(|(list, _)| list.machines.iter().any(|listed| listed.machine == machine));
        if listed {
            return Err(ApiError::conflict("still-listed"));
        }
        let readers = store.admitted_devices(&machine)?;
        store.remove_machine(&machine)?;
        store.log(
            now(),
            "machine.removed",
            Some(&machine),
            None,
            Some(&account),
        )?;
        readers
    };
    hub.wake_devices(&readers);
    hub.wake_account(&account);
    Ok(StatusCode::NO_CONTENT)
}
