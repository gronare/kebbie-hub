use crate::store::{self, Store};
use crate::{Hub, now};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountSwitch {
    Switched,
    NoAccount,
    Owner,
}

pub fn set_account_active(
    store: &Store,
    account: &str,
    active: bool,
    by: &str,
) -> store::Result<AccountSwitch> {
    let Some(row) = store.account(account)? else {
        return Ok(AccountSwitch::NoAccount);
    };
    if row.owner {
        return Ok(AccountSwitch::Owner);
    }
    let at = now();
    store.set_deactivated(account, (!active).then_some(at))?;
    let kind = if active {
        "account.activated"
    } else {
        "account.deactivated"
    };
    store.log(at, kind, None, Some(&format!("{account} by={by}")))?;
    Ok(AccountSwitch::Switched)
}

impl Hub {
    pub fn set_account_active(
        &self,
        account: &str,
        active: bool,
        by: &str,
    ) -> store::Result<AccountSwitch> {
        let switched = set_account_active(&self.store(), account, active, by)?;
        if switched == AccountSwitch::Switched && !active {
            self.wake_account(account);
        }
        Ok(switched)
    }
}
