use std::fmt;

use aes_gcm::aead::{Aead, KeyInit, Payload as AeadInput};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::crypto::{PublicKeys, SecretKeys};
use crate::ids::{Id, Prefix};

pub const WRAP_KEY_INFO: &[u8] = b"kebbie-hub account key v1";
pub const WRAP_AD_DOMAIN: &[u8] = b"kebbie-hub account key wrap v1";
pub const WRAPPER_SIGN_DOMAIN: &[u8] = b"kebbie-hub account key wrapper v1";
pub const MACHINE_LIST_DOMAIN: &[u8] = b"kebbie-hub account machines v1";
pub const SIGNING_FINGERPRINT_DOMAIN: &[u8] = b"kebbie-hub account signing key v1";
pub const ENCRYPTION_FINGERPRINT_DOMAIN: &[u8] = b"kebbie-hub account encryption key v1";
pub const ACCOUNT_COMMIT_DOMAIN: &[u8] = b"kebbie-connect-account-commit-v1";
pub const WRAP_NONCE_LEN: usize = 12;
pub const WRAPPED_LEN: usize = WRAP_NONCE_LEN + 64 + 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WrongAccountKey;

impl fmt::Display for WrongAccountKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the wrapped account key does not open with this passkey")
    }
}

impl std::error::Error for WrongAccountKey {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldTooLong;

impl fmt::Display for FieldTooLong {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a field is longer than a 2-byte length holds")
    }
}

impl std::error::Error for FieldTooLong {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListedMachine {
    pub machine: Id,
    pub keys: PublicKeys,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineList {
    pub counter: u64,
    pub machines: Vec<ListedMachine>,
}

impl MachineList {
    pub fn statement(&self, account: &Id) -> Option<Vec<u8>> {
        let canonical = self
            .machines
            .windows(2)
            .all(|pair| pair[0].machine < pair[1].machine)
            && self
                .machines
                .iter()
                .all(|listed| listed.machine.prefix() == Prefix::Machine);
        if !canonical {
            return None;
        }
        let mut out = MACHINE_LIST_DOMAIN.to_vec();
        id_field(&mut out, account);
        out.extend(self.counter.to_be_bytes());
        out.extend(u32::try_from(self.machines.len()).ok()?.to_be_bytes());
        for listed in &self.machines {
            id_field(&mut out, &listed.machine);
            out.extend(listed.keys.signing);
            out.extend(listed.keys.encryption);
        }
        Some(out)
    }

    pub fn sign(&self, account: &SecretKeys, account_id: &Id) -> Option<[u8; 64]> {
        self.statement(account_id)
            .map(|statement| sign(account, &statement))
    }

    pub fn trusts(&self, machine: &Id, keys: &PublicKeys) -> bool {
        self.machines
            .iter()
            .any(|listed| &listed.machine == machine && &listed.keys == keys)
    }

    pub fn with(&self, machine: Id, keys: PublicKeys) -> MachineList {
        let mut next = self.without(&machine);
        next.machines.push(ListedMachine { machine, keys });
        next.machines.sort_by(|a, b| a.machine.cmp(&b.machine));
        next
    }

    pub fn without(&self, machine: &Id) -> MachineList {
        MachineList {
            counter: self.counter + 1,
            machines: self
                .machines
                .iter()
                .filter(|listed| &listed.machine != machine)
                .cloned()
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountKey {
    pub account: Id,
    pub version: u32,
    pub keys: PublicKeys,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountFingerprints {
    #[serde(with = "crate::crypto::b64_32")]
    pub signing: [u8; 32],
    #[serde(with = "crate::crypto::b64_32")]
    pub encryption: [u8; 32],
}

impl AccountKey {
    pub fn new(account: Id, version: u32, keys: PublicKeys) -> Option<AccountKey> {
        (account.prefix() == Prefix::Account).then_some(AccountKey {
            account,
            version,
            keys,
        })
    }

    pub fn recipient(&self) -> (Id, PublicKeys) {
        (self.account.clone(), self.keys)
    }

    pub fn fingerprints(&self) -> AccountFingerprints {
        AccountFingerprints {
            signing: fingerprint(SIGNING_FINGERPRINT_DOMAIN, self, &self.keys.signing),
            encryption: fingerprint(ENCRYPTION_FINGERPRINT_DOMAIN, self, &self.keys.encryption),
        }
    }

    pub fn verifies_list(&self, list: &MachineList, signature: &[u8; 64]) -> bool {
        list.statement(&self.account)
            .is_some_and(|statement| verify(&self.keys.signing, &statement, signature))
    }

    pub fn verifies_wrapper(
        &self,
        credential: &[u8],
        wrapped: &[u8],
        signature: &[u8; 64],
    ) -> bool {
        wrapper_statement(self, credential, wrapped)
            .is_ok_and(|statement| verify(&self.keys.signing, &statement, signature))
    }
}

fn fingerprint(domain: &[u8], key: &AccountKey, bytes: &[u8; 32]) -> [u8; 32] {
    let mut digest = Sha256::new().chain_update(domain);
    put(&mut digest, &key.account);
    digest
        .chain_update(key.version.to_be_bytes())
        .chain_update(bytes)
        .finalize()
        .into()
}

fn put(digest: &mut Sha256, id: &Id) {
    let mut out = Vec::new();
    id_field(&mut out, id);
    digest.update(out);
}

fn field(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), FieldTooLong> {
    out.extend(
        u16::try_from(bytes.len())
            .map_err(|_| FieldTooLong)?
            .to_be_bytes(),
    );
    out.extend(bytes);
    Ok(())
}

fn id_field(out: &mut Vec<u8>, id: &Id) {
    field(out, id.as_str().as_bytes()).expect("an id is a few dozen bytes");
}

fn verify(signing: &[u8; 32], message: &[u8], signature: &[u8; 64]) -> bool {
    VerifyingKey::from_bytes(signing).is_ok_and(|key| {
        key.verify_strict(message, &Signature::from_bytes(signature))
            .is_ok()
    })
}

pub fn wrap_key(prf_output: &[u8; 32], salt: &[u8; 32]) -> [u8; 32] {
    let mut key = [0u8; 32];
    Hkdf::<Sha256>::new(Some(salt), prf_output)
        .expand(WRAP_KEY_INFO, &mut key)
        .expect("32 bytes is a valid HKDF-SHA256 length");
    key
}

pub fn wrap_associated_data(
    account: &Id,
    credential: &[u8],
    version: u32,
) -> Result<Vec<u8>, FieldTooLong> {
    let mut out = WRAP_AD_DOMAIN.to_vec();
    id_field(&mut out, account);
    field(&mut out, credential)?;
    out.extend(version.to_be_bytes());
    Ok(out)
}

pub struct Unlock<'a> {
    pub account: &'a Id,
    pub credential: &'a [u8],
    pub version: u32,
    pub prf_output: &'a [u8; 32],
    pub salt: &'a [u8; 32],
}

impl Unlock<'_> {
    fn cipher(&self) -> Aes256Gcm {
        Aes256Gcm::new(&Key::<Aes256Gcm>::from(wrap_key(
            self.prf_output,
            self.salt,
        )))
    }

    fn associated_data(&self) -> Result<Vec<u8>, FieldTooLong> {
        wrap_associated_data(self.account, self.credential, self.version)
    }

    pub fn wrap(&self, keys: &SecretKeys) -> Result<Vec<u8>, FieldTooLong> {
        let mut nonce = [0u8; WRAP_NONCE_LEN];
        getrandom::fill(&mut nonce).expect("the system random source failed");
        self.wrap_with_nonce(keys, nonce)
    }

    pub fn wrap_with_nonce(
        &self,
        keys: &SecretKeys,
        nonce: [u8; WRAP_NONCE_LEN],
    ) -> Result<Vec<u8>, FieldTooLong> {
        let aad = self.associated_data()?;
        let (signing, encryption) = keys.to_bytes();
        let plaintext = [signing, encryption].concat();
        let sealed = self
            .cipher()
            .encrypt(
                &Nonce::from(nonce),
                AeadInput {
                    msg: &plaintext,
                    aad: &aad,
                },
            )
            .expect("AES-GCM sealing 64 bytes cannot fail");
        Ok([nonce.as_slice(), &sealed].concat())
    }

    pub fn unwrap(&self, wrapped: &[u8]) -> Result<SecretKeys, WrongAccountKey> {
        if wrapped.len() != WRAPPED_LEN {
            return Err(WrongAccountKey);
        }
        let aad = self.associated_data().map_err(|_| WrongAccountKey)?;
        let nonce: [u8; WRAP_NONCE_LEN] = wrapped[..WRAP_NONCE_LEN]
            .try_into()
            .expect("the length was checked");
        let plaintext = self
            .cipher()
            .decrypt(
                &Nonce::from(nonce),
                AeadInput {
                    msg: &wrapped[WRAP_NONCE_LEN..],
                    aad: &aad,
                },
            )
            .map_err(|_| WrongAccountKey)?;
        let signing: [u8; 32] = plaintext[..32].try_into().map_err(|_| WrongAccountKey)?;
        let encryption: [u8; 32] = plaintext[32..].try_into().map_err(|_| WrongAccountKey)?;
        Ok(SecretKeys::from_bytes(signing, encryption))
    }
}

pub fn wrapper_statement(
    key: &AccountKey,
    credential: &[u8],
    wrapped: &[u8],
) -> Result<Vec<u8>, FieldTooLong> {
    let mut out = WRAPPER_SIGN_DOMAIN.to_vec();
    id_field(&mut out, &key.account);
    field(&mut out, credential)?;
    out.extend(key.version.to_be_bytes());
    out.extend(key.keys.signing);
    out.extend(key.keys.encryption);
    out.extend(Sha256::digest(wrapped));
    Ok(out)
}

pub fn sign_wrapper(
    account: &SecretKeys,
    key: &AccountKey,
    credential: &[u8],
    wrapped: &[u8],
) -> Result<[u8; 64], FieldTooLong> {
    Ok(sign(account, &wrapper_statement(key, credential, wrapped)?))
}

fn sign(keys: &SecretKeys, message: &[u8]) -> [u8; 64] {
    SigningKey::from_bytes(&keys.to_bytes().0)
        .sign(message)
        .to_bytes()
}

pub fn account_commitment(key: &AccountKey, nonce: &[u8; 32], code: &str) -> [u8; 32] {
    let mut digest = Sha256::new().chain_update(ACCOUNT_COMMIT_DOMAIN);
    put(&mut digest, &key.account);
    digest
        .chain_update(key.version.to_be_bytes())
        .chain_update(key.keys.signing)
        .chain_update(key.keys.encryption)
        .chain_update(nonce)
        .chain_update(code.as_bytes())
        .finalize()
        .into()
}

pub struct AccountHandoff {
    key: AccountKey,
    nonce: [u8; 32],
    code: String,
}

impl AccountHandoff {
    pub fn start(key: AccountKey, code: &str) -> AccountHandoff {
        let mut nonce = [0u8; 32];
        getrandom::fill(&mut nonce).expect("the system random source failed");
        AccountHandoff::with_nonce(key, nonce, code)
    }

    pub fn with_nonce(key: AccountKey, nonce: [u8; 32], code: &str) -> AccountHandoff {
        AccountHandoff {
            key,
            nonce,
            code: code.to_string(),
        }
    }

    pub fn commitment(&self) -> [u8; 32] {
        account_commitment(&self.key, &self.nonce, &self.code)
    }

    pub fn reveal(&self) -> (AccountKey, [u8; 32]) {
        (self.key.clone(), self.nonce)
    }
}

pub fn pin_account(
    commitment: &[u8; 32],
    revealed: &AccountKey,
    nonce: &[u8; 32],
    code: &str,
) -> Result<AccountFingerprints, crate::pairing::CommitmentMismatch> {
    if &account_commitment(revealed, nonce, code) != commitment {
        return Err(crate::pairing::CommitmentMismatch);
    }
    Ok(revealed.fingerprints())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(keys: &SecretKeys) -> AccountKey {
        AccountKey::new(Id::account([3; 16]), 1, keys.public()).unwrap()
    }

    #[test]
    fn a_wrap_opens_only_with_its_prf_salt_account_credential_and_version() {
        let keys = SecretKeys::generate();
        let account = Id::account([3; 16]);
        let unlock = Unlock {
            account: &account,
            credential: b"credential",
            version: 1,
            prf_output: &[1; 32],
            salt: &[2; 32],
        };
        let wrapped = unlock.wrap(&keys).unwrap();
        assert_eq!(wrapped.len(), WRAPPED_LEN);
        assert_eq!(unlock.unwrap(&wrapped).unwrap().public(), keys.public());
        let other = Id::account([4; 16]);
        for changed in [
            Unlock {
                prf_output: &[9; 32],
                ..unlock
            },
            Unlock {
                salt: &[9; 32],
                ..unlock
            },
            Unlock {
                account: &other,
                ..unlock
            },
            Unlock {
                credential: b"another",
                ..unlock
            },
            Unlock {
                version: 2,
                ..unlock
            },
        ] {
            assert_eq!(changed.unwrap(&wrapped).err(), Some(WrongAccountKey));
        }
        assert_eq!(unlock.unwrap(&wrapped[1..]).err(), Some(WrongAccountKey));
    }

    #[test]
    fn statements_verify_only_against_what_was_signed() {
        let keys = SecretKeys::generate();
        let account = key(&keys);
        let wrapper = sign_wrapper(&keys, &account, b"credential", b"wrapped").unwrap();
        assert!(account.verifies_wrapper(b"credential", b"wrapped", &wrapper));
        assert!(!account.verifies_wrapper(b"credential", b"other", &wrapper));
        assert!(!key(&SecretKeys::generate()).verifies_wrapper(
            b"credential",
            b"wrapped",
            &wrapper
        ));
    }

    #[test]
    fn a_machine_list_is_trusted_only_as_signed_and_canonical() {
        let keys = SecretKeys::generate();
        let account = key(&keys);
        let first = Id::ulid(Prefix::Machine, 1, [5; 10]).unwrap();
        let second = Id::ulid(Prefix::Machine, 2, [6; 10]).unwrap();
        let (first_keys, second_keys) = (
            SecretKeys::generate().public(),
            SecretKeys::generate().public(),
        );
        let list = MachineList::default()
            .with(second.clone(), second_keys)
            .with(first.clone(), first_keys);
        assert_eq!(list.counter, 2);
        let signature = list.sign(&keys, &account.account).unwrap();
        assert!(account.verifies_list(&list, &signature));
        assert!(list.trusts(&first, &first_keys) && !list.trusts(&first, &second_keys));
        let removed = list.without(&second);
        assert_eq!(removed.counter, 3);
        assert!(!removed.trusts(&second, &second_keys));
        assert!(!account.verifies_list(&removed, &signature));
        let mut replayed = removed.clone();
        replayed.counter = list.counter;
        assert!(
            !account.verifies_list(&replayed, &signature),
            "the counter is signed"
        );
        let mut shuffled = list.clone();
        shuffled.machines.reverse();
        assert!(shuffled.statement(&account.account).is_none());
        assert!(!account.verifies_list(&shuffled, &signature));
        let doubled = MachineList {
            counter: 9,
            machines: vec![list.machines[0].clone(), list.machines[0].clone()],
        };
        assert!(doubled.sign(&keys, &account.account).is_none());
    }

    #[test]
    fn a_credential_longer_than_a_two_byte_length_is_refused_not_truncated() {
        let keys = SecretKeys::generate();
        let account = key(&keys);
        let long = vec![7u8; 65_536];
        assert_eq!(
            wrap_associated_data(&account.account, &long, 1),
            Err(FieldTooLong)
        );
        assert_eq!(
            sign_wrapper(&keys, &account, &long, b"wrapped"),
            Err(FieldTooLong)
        );
        let unlock = Unlock {
            account: &account.account,
            credential: &long,
            version: 1,
            prf_output: &[1; 32],
            salt: &[2; 32],
        };
        assert_eq!(unlock.wrap(&keys), Err(FieldTooLong));
        let short = Unlock {
            credential: &long[..65_535],
            ..unlock
        };
        assert!(short.wrap(&keys).is_ok());
    }

    #[test]
    fn a_handoff_pins_only_the_keys_it_committed_to() {
        let keys = SecretKeys::generate();
        let handoff = AccountHandoff::start(key(&keys), "secret");
        let (revealed, nonce) = handoff.reveal();
        let pinned = pin_account(&handoff.commitment(), &revealed, &nonce, "secret").unwrap();
        assert_eq!(pinned, revealed.fingerprints());
        assert!(pin_account(&handoff.commitment(), &revealed, &nonce, "guess").is_err());
        let swapped = key(&SecretKeys::generate());
        assert!(pin_account(&handoff.commitment(), &swapped, &nonce, "secret").is_err());
        assert!(
            AccountKey::new(
                Id::ulid(Prefix::Device, 1, [1; 10]).unwrap(),
                1,
                keys.public()
            )
            .is_none()
        );
    }
}
