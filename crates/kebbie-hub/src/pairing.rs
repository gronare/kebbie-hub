use kebbie_hub_api::{Bytes32, PairingState, PairingView};
use kebbie_hub_protocol::AccountKey;
use serde::Deserialize;

use crate::store::Pairing;

#[derive(Clone, Copy)]
pub enum Side {
    Machine,
    Account,
}

#[derive(Deserialize)]
pub struct Watch {
    #[serde(default)]
    pub wait: u64,
    pub seen: Option<PairingState>,
}

impl Watch {
    pub fn changed(&self, state: PairingState) -> bool {
        state.is_final() || self.seen != Some(state)
    }
}

pub fn view(pairing: &Pairing, side: Side, now: u64) -> PairingView {
    let shows_reply = matches!(side, Side::Account);
    let shows_reveal = matches!(side, Side::Machine);
    let account_key = pairing
        .account_keys
        .zip(pairing.account_key_version)
        .and_then(|(keys, version)| AccountKey::new(pairing.account.clone(), version, keys))
        .filter(|_| shows_reveal);
    PairingView {
        account_nonce: pairing.account_nonce.filter(|_| shows_reveal).map(Bytes32),
        account_key,
        account: pairing.account.clone(),
        pairing: pairing.id.clone(),
        state: pairing.state_at(now),
        reason: pairing.reason,
        expires_at: pairing.expires_at,
        machine: pairing.machine.clone(),
        commitment: Some(pairing.commitment).filter(|_| shows_reveal).map(Bytes32),
        machine_keys: pairing.machine_keys.filter(|_| shows_reply),
        machine_nonce: pairing.machine_nonce.filter(|_| shows_reply).map(Bytes32),
        machine_name: None,
    }
}
