use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use axum::response::{IntoResponse, Response};
use kebbie_hub_api::{
    AccountPlan, AdminAccount, AdminAccounts, AdminMachine, AdminMachines, AdminOverview,
    AdminReport, AdminReports, AdminSignal, AdminSignals, BillingAccount, BillingAccountDetail,
    BillingAccounts, BillingMachine, Bytes32, InviteLink, PlanUpdate,
};
use kebbie_hub_protocol::Id;
use serde::Deserialize;

use axum::http::HeaderMap;

use crate::auth::{AdminAuth, FreshOwner, OwnerAuth, SameOrigin, require_fresh};
use crate::error::ApiError;
use crate::store::{AdminScope, KeyStatus};
use crate::{Hub, MAX_STORED_INTEGER, mint_invite, now};

const SIGNAL_DAYS: u64 = 7;
const ACCOUNT_PAGE: u64 = 500;
const ACTIVE_SECONDS: u64 = 7 * 86_400;
const BILLING_ACTOR: &str = "billing-token";
const MAX_SIGNAL_DAYS: u64 = 180;

pub async fn overview(
    State(hub): State<Arc<Hub>>,
    _: OwnerAuth,
) -> Result<Json<AdminOverview>, ApiError> {
    let store = hub.store();
    let (accounts, machines, devices) = store.counts()?;
    let today = now() / 86_400;
    let signals_this_week = store
        .signal_rows(today.saturating_sub(SIGNAL_DAYS - 1))?
        .iter()
        .map(|row| row.count)
        .sum();
    Ok(Json(AdminOverview {
        accounts,
        machines,
        devices,
        reports: store.allowed_unacked_count()?,
        quarantined: store.quarantined_count()?,
        signals_this_week,
    }))
}

pub async fn accounts(
    State(hub): State<Arc<Hub>>,
    _: OwnerAuth,
) -> Result<Json<AdminAccounts>, ApiError> {
    let at = now();
    let rows = hub.store().admin_accounts()?;
    let accounts = rows
        .into_iter()
        .map(|row| AdminAccount {
            plan: hub.plan_view(&row.account, at),
            account: row.account.id,
            handle: row.account.handle,
            email: row.account.email,
            owner: row.account.owner,
            created_at: row.account.created_at,
            deactivated_at: row.account.deactivated_at,
            machines: row.machines,
            devices: row.devices,
            last_seen: row.last_seen,
        })
        .collect();
    Ok(Json(AdminAccounts { accounts }))
}

fn set_deactivated(
    hub: &Hub,
    owner: &str,
    account: &str,
    at: Option<u64>,
) -> Result<StatusCode, ApiError> {
    let store = hub.store();
    let row = store.account(account)?.ok_or_else(ApiError::not_found)?;
    if row.owner {
        return Err(ApiError::conflict("owner-stays"));
    }
    store.set_deactivated(account, at)?;
    let kind = if at.is_some() {
        "account.deactivated"
    } else {
        "account.activated"
    };
    store.log(
        now(),
        kind,
        None,
        None,
        Some(&format!("{account} by={owner}")),
    )?;
    Ok(StatusCode::NO_CONTENT)
}

fn deactivate_account(hub: &Hub, by: &str, account: &str) -> Result<StatusCode, ApiError> {
    let status = set_deactivated(hub, by, account, Some(now()))?;
    let devices = hub
        .store()
        .devices_of_account(account)?
        .into_iter()
        .map(|row| row.id)
        .collect::<Vec<_>>();
    hub.wake_devices(&devices);
    hub.wake_account(account);
    Ok(status)
}

pub async fn deactivate(
    State(hub): State<Arc<Hub>>,
    _: SameOrigin,
    FreshOwner(owner): FreshOwner,
    Path(account): Path<String>,
) -> Result<StatusCode, ApiError> {
    deactivate_account(&hub, &owner, &account)
}

pub async fn activate(
    State(hub): State<Arc<Hub>>,
    _: SameOrigin,
    OwnerAuth(owner): OwnerAuth,
    Path(account): Path<String>,
) -> Result<StatusCode, ApiError> {
    set_deactivated(&hub, &owner, &account, None)
}

pub async fn billing_deactivate(
    State(hub): State<Arc<Hub>>,
    admin: AdminAuth,
    Path(account): Path<String>,
) -> Result<StatusCode, ApiError> {
    admin.require(AdminScope::Billing)?;
    deactivate_account(&hub, BILLING_ACTOR, &account)
}

pub async fn billing_activate(
    State(hub): State<Arc<Hub>>,
    admin: AdminAuth,
    Path(account): Path<String>,
) -> Result<StatusCode, ApiError> {
    admin.require(AdminScope::Billing)?;
    set_deactivated(&hub, BILLING_ACTOR, &account, None)
}

pub async fn invite(
    State(hub): State<Arc<Hub>>,
    _: SameOrigin,
    OwnerAuth(owner): OwnerAuth,
) -> Result<Json<InviteLink>, ApiError> {
    let ttl = hub.config().invite_ttl;
    let store = hub.store();
    let code = mint_invite(&store, ttl)?;
    let at = now();
    store.log(at, "invite.created", None, None, Some(&owner))?;
    Ok(Json(InviteLink {
        url: format!("{}/signup?invite={code}", hub.origin()),
        code,
        expires_at: at + ttl,
    }))
}

pub async fn machines(
    State(hub): State<Arc<Hub>>,
    _: OwnerAuth,
) -> Result<Json<AdminMachines>, ApiError> {
    let store = hub.store();
    let machines = store
        .machines()?
        .into_iter()
        .map(|row| {
            Ok(AdminMachine {
                devices: store.admitted_count(&row.id)?,
                machine: row.id,
                name: row.name,
                owner: row.owner,
                joined_at: row.joined_at,
                last_seen: row.last_seen,
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    Ok(Json(AdminMachines { machines }))
}

pub async fn remove_machine(
    State(hub): State<Arc<Hub>>,
    _: SameOrigin,
    FreshOwner(owner): FreshOwner,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let machine = Id::parse(&id).map_err(|_| ApiError::not_found())?;
    let (readers, account) = {
        let store = hub.store();
        let readers = store.admitted_devices(&machine)?;
        let account = store.machine_owner(&machine)?;
        if !store.remove_machine(&machine)? {
            return Err(ApiError::not_found());
        }
        store.log(now(), "machine.removed", Some(&machine), None, Some(&owner))?;
        (readers, account)
    };
    hub.wake_devices(&readers);
    if let Some(account) = account {
        hub.wake_account(&account);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct ReportsQuery {
    #[serde(default)]
    quarantine: bool,
}

pub async fn reports(
    State(hub): State<Arc<Hub>>,
    _: OwnerAuth,
    Query(query): Query<ReportsQuery>,
) -> Result<Json<AdminReports>, ApiError> {
    let status = if query.quarantine {
        KeyStatus::Quarantine
    } else {
        KeyStatus::Allowed
    };
    let reports = hub
        .store()
        .reports(status, 0, !query.quarantine, crate::report::INBOX_PAGE)?
        .into_iter()
        .map(|stored| AdminReport {
            id: stored.id,
            key: Bytes32(stored.key),
            status: stored.status.name().to_string(),
            received_at: stored.received_at,
            acked_at: stored.acked_at,
            report: stored.report,
        })
        .collect();
    Ok(Json(AdminReports { reports }))
}

pub async fn ack_report(
    State(hub): State<Arc<Hub>>,
    _: SameOrigin,
    _: OwnerAuth,
    Path(id): Path<u64>,
) -> Result<StatusCode, ApiError> {
    if id <= MAX_STORED_INTEGER && hub.store().ack_report(id, now())? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found())
    }
}

pub async fn decide_key(
    State(hub): State<Arc<Hub>>,
    _: SameOrigin,
    _: OwnerAuth,
    headers: HeaderMap,
    Path((key, decision)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let key: [u8; 32] = URL_SAFE_NO_PAD
        .decode(&key)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(ApiError::not_found)?;
    let status = match decision.as_str() {
        "allow" => KeyStatus::Allowed,
        "block" => {
            require_fresh(&hub, &headers)?;
            KeyStatus::Blocked
        }
        _ => return Err(ApiError::not_found()),
    };
    hub.store().set_key_status(&key, status, now())?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct SignalsQuery {
    #[serde(default)]
    days: Option<u64>,
}

pub async fn signals(
    State(hub): State<Arc<Hub>>,
    _: OwnerAuth,
    Query(query): Query<SignalsQuery>,
) -> Result<Json<AdminSignals>, ApiError> {
    let days = query.days.unwrap_or(SIGNAL_DAYS).clamp(1, MAX_SIGNAL_DAYS);
    let today = now() / 86_400;
    let signals = hub
        .store()
        .signal_rows(today.saturating_sub(days - 1))?
        .into_iter()
        .map(|row| AdminSignal {
            day: row.day,
            version: row.version,
            commit: row.commit,
            os: row.os,
            arch: row.arch,
            component: row.component,
            kind: row.kind,
            vetted: row.vetted,
            count: row.count,
        })
        .collect();
    Ok(Json(AdminSignals { signals }))
}

fn account_plan(hub: &Hub, account: &str) -> Result<AccountPlan, ApiError> {
    let row = hub
        .store()
        .account(account)?
        .ok_or_else(ApiError::not_found)?;
    Ok(AccountPlan {
        plan: hub.plan_view(&row, now()),
        account: row.id,
        handle: row.handle,
        email: row.email,
    })
}

#[derive(Deserialize)]
pub struct AccountsQuery {
    email: Option<String>,
    after: Option<String>,
    limit: Option<u64>,
}

pub async fn billing_accounts(
    State(hub): State<Arc<Hub>>,
    admin: AdminAuth,
    Query(query): Query<AccountsQuery>,
) -> Result<Response, ApiError> {
    admin.require(AdminScope::Billing)?;
    match query.email {
        Some(email) => Ok(Json(billing_lookup(&hub, &email)?).into_response()),
        None => Ok(Json(account_page(&hub, query.after.as_deref(), query.limit)?).into_response()),
    }
}

fn billing_lookup(hub: &Hub, email: &str) -> Result<AccountPlan, ApiError> {
    let email = email.trim().to_ascii_lowercase();
    let account = hub
        .store()
        .account_by_email(&email)?
        .ok_or_else(ApiError::not_found)?;
    account_plan(hub, &account)
}

fn account_page(hub: &Hub, after: Option<&str>, limit: Option<u64>) -> Result<BillingAccounts, ApiError> {
    let limit = limit.unwrap_or(ACCOUNT_PAGE);
    if !(1..=ACCOUNT_PAGE).contains(&limit) {
        return Err(ApiError::bad_request("limit-out-of-range"));
    }
    let at = now();
    let mut rows = hub
        .store()
        .account_page(after, limit + 1, at.saturating_sub(ACTIVE_SECONDS))?
        .ok_or_else(|| ApiError::bad_request("unknown-after"))?;
    let more = rows.len() as u64 > limit;
    rows.truncate(limit as usize);
    let accounts: Vec<BillingAccount> = rows
        .into_iter()
        .map(|row| BillingAccount {
            plan: hub.plan_view(&row.account, at),
            account: row.account.id,
            handle: row.account.handle,
            github: row.account.github_login,
            created_at: row.account.created_at,
            deactivated_at: row.account.deactivated_at,
            machines: row.machines,
            active_machines: row.active_machines,
            last_seen: row.last_seen,
        })
        .collect();
    let next = more
        .then(|| accounts.last().map(|row| row.account.clone()))
        .flatten();
    Ok(BillingAccounts { accounts, next })
}

pub async fn billing_account(
    State(hub): State<Arc<Hub>>,
    admin: AdminAuth,
    Path(account): Path<String>,
) -> Result<Json<BillingAccountDetail>, ApiError> {
    admin.require(AdminScope::Billing)?;
    let row = hub
        .store()
        .account_detail(&account)?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(BillingAccountDetail {
        plan: hub.plan_view(&row.account, now()),
        account: row.account.id,
        handle: row.account.handle,
        github: row.account.github_login,
        created_at: row.account.created_at,
        created_via: row.account.created_via,
        deactivated_at: row.account.deactivated_at,
        passkeys: row.passkeys,
        last_seen: row.last_seen,
        machines: row
            .machines
            .into_iter()
            .map(|machine| BillingMachine {
                machine: machine.id,
                name: machine.name,
                joined_at: machine.joined_at,
                last_seen: machine.last_seen,
                last_board_at: machine.last_board_at,
            })
            .collect(),
    }))
}

pub async fn set_plan(
    State(hub): State<Arc<Hub>>,
    admin: AdminAuth,
    Path(account): Path<String>,
    Json(update): Json<PlanUpdate>,
) -> Result<Json<AccountPlan>, ApiError> {
    admin.require(AdminScope::Billing)?;
    if update.until.is_some_and(|until| until > MAX_STORED_INTEGER) {
        return Err(ApiError::bad_request("until-out-of-range"));
    }
    {
        let store = hub.store();
        if !store.set_plan(&account, update.status, update.until)? {
            return Err(ApiError::not_found());
        }
        store.log(
            now(),
            "account.plan",
            None,
            None,
            Some(&format!("{account} {}", update.status.name())),
        )?;
    }
    Ok(Json(account_plan(&hub, &account)?))
}
