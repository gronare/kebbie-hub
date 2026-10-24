use std::fmt;

use sha2::{Digest, Sha256};

use crate::crypto::PublicKeys;

const COMMIT_DOMAIN: &[u8] = b"kelpie-pair-commit-v1";
const SAS_DOMAIN: &[u8] = b"kelpie-pair-v1";

pub fn commitment(device: &PublicKeys, device_nonce: &[u8; 32], code: &str) -> [u8; 32] {
    Sha256::new()
        .chain_update(COMMIT_DOMAIN)
        .chain_update(device.signing)
        .chain_update(device.encryption)
        .chain_update(device_nonce)
        .chain_update(code.as_bytes())
        .finalize()
        .into()
}

pub fn comparison_code(
    machine: &PublicKeys,
    device: &PublicKeys,
    machine_nonce: &[u8; 32],
    device_nonce: &[u8; 32],
    code: &str,
) -> String {
    let digest = Sha256::new()
        .chain_update(SAS_DOMAIN)
        .chain_update(machine.signing)
        .chain_update(machine.encryption)
        .chain_update(device.signing)
        .chain_update(device.encryption)
        .chain_update(machine_nonce)
        .chain_update(device_nonce)
        .chain_update(code.as_bytes())
        .finalize();
    let number = u32::from_be_bytes(
        digest[..4]
            .try_into()
            .expect("a SHA-256 digest has 32 bytes"),
    ) % 1_000_000;
    format!("{:03} {:03}", number / 1_000, number % 1_000)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommitmentMismatch;

impl fmt::Display for CommitmentMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the device revealed keys, a nonce or a code it did not commit to")
    }
}

impl std::error::Error for CommitmentMismatch {}

pub struct DevicePairing {
    keys: PublicKeys,
    nonce: [u8; 32],
    code: String,
}

impl DevicePairing {
    pub fn start(keys: PublicKeys, code: &str) -> DevicePairing {
        DevicePairing::with_nonce(keys, random_nonce(), code)
    }

    pub fn with_nonce(keys: PublicKeys, nonce: [u8; 32], code: &str) -> DevicePairing {
        DevicePairing {
            keys,
            nonce,
            code: code.to_string(),
        }
    }

    pub fn commitment(&self) -> [u8; 32] {
        commitment(&self.keys, &self.nonce, &self.code)
    }

    pub fn reveal(&self) -> (PublicKeys, [u8; 32]) {
        (self.keys, self.nonce)
    }

    pub fn comparison_code(&self, machine: &PublicKeys, machine_nonce: &[u8; 32]) -> String {
        comparison_code(machine, &self.keys, machine_nonce, &self.nonce, &self.code)
    }
}

pub struct MachinePairing {
    keys: PublicKeys,
    nonce: [u8; 32],
    commitment: [u8; 32],
    code: String,
}

impl MachinePairing {
    pub fn answer(keys: PublicKeys, commitment: [u8; 32], code: &str) -> MachinePairing {
        MachinePairing::with_nonce(keys, commitment, random_nonce(), code)
    }

    pub fn with_nonce(
        keys: PublicKeys,
        commitment: [u8; 32],
        nonce: [u8; 32],
        code: &str,
    ) -> MachinePairing {
        MachinePairing {
            keys,
            nonce,
            commitment,
            code: code.to_string(),
        }
    }

    pub fn reply(&self) -> (PublicKeys, [u8; 32]) {
        (self.keys, self.nonce)
    }

    pub fn comparison_code(
        &self,
        device: &PublicKeys,
        device_nonce: &[u8; 32],
    ) -> Result<String, CommitmentMismatch> {
        if commitment(device, device_nonce, &self.code) != self.commitment {
            return Err(CommitmentMismatch);
        }
        Ok(comparison_code(
            &self.keys,
            device,
            &self.nonce,
            device_nonce,
            &self.code,
        ))
    }
}

fn random_nonce() -> [u8; 32] {
    let mut nonce = [0u8; 32];
    getrandom::fill(&mut nonce).expect("the system random source failed");
    nonce
}
