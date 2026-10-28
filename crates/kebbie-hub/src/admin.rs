use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use kebbie_hub_api::{
    AccountPlan, BillingAccount, BillingAccountDetail, BillingAccounts, BillingMachine, PlanUpdate,
};
use serde::Deserialize;

use crate::auth::AdminAuth;
use crate::error::ApiError;
use crate::store::AdminScope;
use crate::{Hub, MAX_STORED_INTEGER, now};

const ACCOUNT_PAGE: u64 = 500;
const ACTIVE_SECONDS: u64 = 7 * 86_400;
const BILLING_ACTOR: &str = "billing-token";

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
