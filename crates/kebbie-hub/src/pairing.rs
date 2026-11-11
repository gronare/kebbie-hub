use kebbie_hub_api::{Bytes32, PairingState, PairingView};
use kebbie_hub_protocol::{AccountKey, Id};
use serde::Deserialize;

use crate::store::Pairing;

#[derive(Clone, Copy)]
pub enum Side {
    Machine,
    Device,
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
    let (shows_reply, shows_reveal) = match side {
        Side::Machine => (false, true),
        Side::Device => (true, false),
    };
    let account = pairing
        .account
        .as_deref()
        .and_then(|account| Id::parse(account).ok());
    let device_side = account.is_none();
    let account_key = account
        .clone()
        .zip(pairing.device_keys.zip(pairing.account_key_version))
        .and_then(|(account, (keys, version))| AccountKey::new(account, version, keys))
        .filter(|_| shows_reveal);
    PairingView {
        account_nonce: pairing
            .device_nonce
            .filter(|_| shows_reveal && !device_side)
            .map(Bytes32),
        account_key,
        account,
        pairing: pairing.id.clone(),
        state: pairing.state_at(now),
        reason: pairing.reason,
        expires_at: pairing.expires_at,
        machine: pairing.machine.clone(),
        device: pairing.device.clone(),
        commitment: pairing.commitment.filter(|_| shows_reveal).map(Bytes32),
        machine_keys: pairing.machine_keys.filter(|_| shows_reply),
        machine_nonce: pairing.machine_nonce.filter(|_| shows_reply).map(Bytes32),
        device_keys: pairing.device_keys.filter(|_| shows_reveal && device_side),
        device_nonce: pairing
            .device_nonce
            .filter(|_| shows_reveal && device_side)
            .map(Bytes32),
        machine_name: None,
    }
}
