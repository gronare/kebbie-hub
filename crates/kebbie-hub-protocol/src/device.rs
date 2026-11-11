use std::fmt;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::account::AccountKey;
use crate::crypto::SecretKeys;
use crate::ids::{Id, Prefix};

pub const DEVICE_CERT_DOMAIN: &[u8] = b"kebbie-hub device certificate v1";
pub const DEVICE_FINGERPRINT_DOMAIN: &[u8] = b"kebbie-hub device key v1";
pub const MAX_DEVICE_NAME_CHARS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeviceKind {
    Web,
}

impl DeviceKind {
    pub fn name(self) -> &'static str {
        match self {
            DeviceKind::Web => "web",
        }
    }
}

#[derive(Clone)]
pub struct DeviceKey {
    signing: SigningKey,
}

impl DeviceKey {
    pub fn generate() -> DeviceKey {
        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret).expect("the system random source failed");
        DeviceKey::from_bytes(secret)
    }

    pub fn from_bytes(secret: [u8; 32]) -> DeviceKey {
        DeviceKey {
            signing: SigningKey::from_bytes(&secret),
        }
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        self.signing.to_bytes()
    }

    pub fn public(&self) -> [u8; 32] {
        self.signing.verifying_key().to_bytes()
    }

    pub(crate) fn signing(&self) -> &SigningKey {
        &self.signing
    }
}

impl fmt::Debug for DeviceKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeviceKey")
            .field("fingerprint", &device_fingerprint(&self.public()))
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceCert {
    pub account: Id,
    pub device: Id,
    pub name: String,
    pub kind: DeviceKind,
    #[serde(with = "crate::crypto::b64_32")]
    pub signing: [u8; 32],
    pub created_at: u64,
    #[serde(with = "crate::crypto::b64_64")]
    pub sig: [u8; 64],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadCert(pub &'static str);

impl fmt::Display for BadCert {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "not a device certificate: {}", self.0)
    }
}

impl std::error::Error for BadCert {}

impl DeviceCert {
    pub fn issue(
        account: &SecretKeys,
        account_id: &Id,
        device: Id,
        name: &str,
        kind: DeviceKind,
        signing: [u8; 32],
        created_at: u64,
    ) -> Result<DeviceCert, BadCert> {
        let mut cert = DeviceCert {
            account: account_id.clone(),
            device,
            name: name.to_string(),
            kind,
            signing,
            created_at,
            sig: [0; 64],
        };
        let statement = cert.statement()?;
        cert.sig = SigningKey::from_bytes(&account.to_bytes().0)
            .sign(&statement)
            .to_bytes();
        Ok(cert)
    }

    pub fn check(&self) -> Result<(), BadCert> {
        if self.account.prefix() != Prefix::Account {
            return Err(BadCert("account is not an account id"));
        }
        if self.device.prefix() != Prefix::Device {
            return Err(BadCert("device is not a device id"));
        }
        if !is_device_name(&self.name) {
            return Err(BadCert(
                "the name is empty, too long or holds control characters",
            ));
        }
        Ok(())
    }

    pub fn statement(&self) -> Result<Vec<u8>, BadCert> {
        self.check()?;
        let mut out = DEVICE_CERT_DOMAIN.to_vec();
        self.put_body(&mut out);
        Ok(out)
    }

    pub(crate) fn put_body(&self, out: &mut Vec<u8>) {
        for text in [
            self.account.as_str(),
            self.device.as_str(),
            self.name.as_str(),
            self.kind.name(),
        ] {
            out.extend((text.len() as u16).to_be_bytes());
            out.extend(text.as_bytes());
        }
        out.extend(self.signing);
        out.extend(self.created_at.to_be_bytes());
    }

    pub fn fingerprint(&self) -> String {
        device_fingerprint(&self.signing)
    }
}

impl AccountKey {
    pub fn verifies_device(&self, cert: &DeviceCert) -> bool {
        cert.account == self.account
            && cert.statement().is_ok_and(|statement| {
                VerifyingKey::from_bytes(&self.keys.signing).is_ok_and(|key| {
                    key.verify_strict(&statement, &Signature::from_bytes(&cert.sig))
                        .is_ok()
                })
            })
    }
}

fn is_device_name(name: &str) -> bool {
    (1..=MAX_DEVICE_NAME_CHARS).contains(&name.chars().count())
        && !name.chars().any(char::is_control)
}

pub fn device_fingerprint(signing: &[u8; 32]) -> String {
    let digest = Sha256::new()
        .chain_update(DEVICE_FINGERPRINT_DOMAIN)
        .chain_update(signing)
        .finalize();
    format!(
        "{:02X}{:02X}-{:02X}{:02X}",
        digest[0], digest[1], digest[2], digest[3]
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedDevice(DeviceCert);

impl VerifiedDevice {
    pub(crate) fn new(cert: DeviceCert) -> VerifiedDevice {
        VerifiedDevice(cert)
    }

    pub fn id(&self) -> &Id {
        &self.0.device
    }

    pub fn signing(&self) -> &[u8; 32] {
        &self.0.signing
    }

    pub fn fingerprint(&self) -> String {
        self.0.fingerprint()
    }

    pub fn cert(&self) -> &DeviceCert {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trust {
    Approved,
    Waiting,
    Denied,
}

pub trait DeviceTrust {
    fn trust(&self, device: &VerifiedDevice, now: u64) -> Trust;
}

pub struct TrustAccount;

impl DeviceTrust for TrustAccount {
    fn trust(&self, _: &VerifiedDevice, _: u64) -> Trust {
        Trust::Approved
    }
}

impl<F: Fn(&VerifiedDevice, u64) -> Trust> DeviceTrust for F {
    fn trust(&self, device: &VerifiedDevice, now: u64) -> Trust {
        self(device, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account() -> (SecretKeys, AccountKey) {
        let keys = SecretKeys::generate();
        let key = AccountKey::new(Id::account([3; 16]), 1, keys.public()).unwrap();
        (keys, key)
    }

    fn device(name: &str) -> Id {
        Id::ulid(Prefix::Device, 1, [name.len() as u8; 10]).unwrap()
    }

    #[test]
    fn a_cert_verifies_only_as_its_account_signed_it() {
        let (keys, key) = account();
        let phone = DeviceKey::generate();
        let cert = DeviceCert::issue(
            &keys,
            &key.account,
            device("phone"),
            "iPhone 16",
            DeviceKind::Web,
            phone.public(),
            1_000,
        )
        .unwrap();
        assert!(key.verifies_device(&cert));
        for changed in [
            DeviceCert {
                name: "iPad".into(),
                ..cert.clone()
            },
            DeviceCert {
                created_at: 1_001,
                ..cert.clone()
            },
            DeviceCert {
                signing: DeviceKey::generate().public(),
                ..cert.clone()
            },
            DeviceCert {
                device: device("tablet"),
                ..cert.clone()
            },
        ] {
            assert!(!key.verifies_device(&changed), "{changed:?}");
        }
        let (_, stranger) = account();
        assert!(
            !stranger.verifies_device(&cert),
            "another key under the same account id"
        );
        let (other_keys, _) = account();
        let other = AccountKey::new(Id::account([4; 16]), 1, other_keys.public()).unwrap();
        let theirs = DeviceCert::issue(
            &other_keys,
            &other.account,
            device("phone"),
            "iPhone 16",
            DeviceKind::Web,
            phone.public(),
            1_000,
        )
        .unwrap();
        assert!(other.verifies_device(&theirs) && !key.verifies_device(&theirs));
    }

    #[test]
    fn a_cert_names_an_account_a_device_and_a_short_printable_name() {
        let (keys, key) = account();
        let signing = DeviceKey::generate().public();
        let issue = |device: Id, name: &str| {
            DeviceCert::issue(
                &keys,
                &key.account,
                device,
                name,
                DeviceKind::Web,
                signing,
                1,
            )
        };
        assert!(issue(device("phone"), &"é".repeat(MAX_DEVICE_NAME_CHARS)).is_ok());
        assert!(issue(device("phone"), &"é".repeat(MAX_DEVICE_NAME_CHARS + 1)).is_err());
        assert!(issue(device("phone"), "").is_err());
        assert!(issue(device("phone"), "iPhone\n16").is_err());
        assert!(issue(Id::ulid(Prefix::Machine, 1, [1; 10]).unwrap(), "iPhone").is_err());
        assert!(
            DeviceCert::issue(
                &keys,
                &Id::ulid(Prefix::Machine, 1, [1; 10]).unwrap(),
                device("phone"),
                "iPhone",
                DeviceKind::Web,
                signing,
                1
            )
            .is_err()
        );
    }

    #[test]
    fn a_fingerprint_is_four_bytes_of_the_keys_digest_in_two_groups() {
        let print = device_fingerprint(&[7; 32]);
        assert_eq!(print.len(), 9);
        assert!(
            print
                .chars()
                .enumerate()
                .all(|(index, char)| if index == 4 {
                    char == '-'
                } else {
                    char.is_ascii_hexdigit() && !char.is_ascii_lowercase()
                })
        );
        assert_ne!(print, device_fingerprint(&[8; 32]));
    }
}
