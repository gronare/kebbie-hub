use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

use crate::crypto::PublicKeys;

pub const CONNECT_FINGERPRINT_DOMAIN: &[u8] = b"kelpie-connect-machine-v1";

pub fn machine_fingerprint(keys: &PublicKeys) -> [u8; 32] {
    Sha256::new()
        .chain_update(CONNECT_FINGERPRINT_DOMAIN)
        .chain_update(keys.signing)
        .chain_update(keys.encryption)
        .finalize()
        .into()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectSecret {
    secret: [u8; 32],
    fingerprint: [u8; 32],
}

impl ConnectSecret {
    pub fn new(machine: &PublicKeys) -> ConnectSecret {
        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret).expect("the system random source failed");
        ConnectSecret::with_secret(machine, secret)
    }

    pub fn with_secret(machine: &PublicKeys, secret: [u8; 32]) -> ConnectSecret {
        ConnectSecret {
            secret,
            fingerprint: machine_fingerprint(machine),
        }
    }

    pub fn pairing_code(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.secret)
    }

    pub fn fragment(&self) -> String {
        format!(
            "k={}&m={}",
            self.pairing_code(),
            URL_SAFE_NO_PAD.encode(self.fingerprint)
        )
    }

    pub fn link(&self, verification_uri_complete: &str) -> String {
        format!("{verification_uri_complete}#{}", self.fragment())
    }

    pub fn parse(fragment: &str) -> Option<ConnectSecret> {
        let mut secret = None;
        let mut fingerprint = None;
        for pair in fragment.trim_start_matches('#').split('&') {
            let decoded = |value: &str| -> Option<[u8; 32]> {
                URL_SAFE_NO_PAD.decode(value).ok()?.try_into().ok()
            };
            match pair.split_once('=') {
                Some(("k", value)) => secret = decoded(value),
                Some(("m", value)) => fingerprint = decoded(value),
                _ => {}
            }
        }
        Some(ConnectSecret {
            secret: secret?,
            fingerprint: fingerprint?,
        })
    }

    pub fn names(&self, machine: &PublicKeys) -> bool {
        machine_fingerprint(machine) == self.fingerprint
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SecretKeys;

    #[test]
    fn a_fragment_parses_back_and_names_only_its_machine() {
        let machine = SecretKeys::generate().public();
        let secret = ConnectSecret::new(&machine);
        let parsed = ConnectSecret::parse(&format!("#{}", secret.fragment())).unwrap();
        assert_eq!(parsed, secret);
        assert!(parsed.names(&machine));
        assert!(!parsed.names(&SecretKeys::generate().public()));
        assert_eq!(ConnectSecret::parse("k=AAAA"), None);
        assert_eq!(parsed.pairing_code().len(), 43);
    }
}
